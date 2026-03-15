package main

import (
	"context"
	"flag"
	"fmt"
	"io/fs"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/akazakov/openwrt-shaper/internal/api"
	"github.com/akazakov/openwrt-shaper/internal/config"
	"github.com/akazakov/openwrt-shaper/internal/dish"
	"github.com/akazakov/openwrt-shaper/internal/engine"
	"github.com/akazakov/openwrt-shaper/internal/netctl"
	"github.com/akazakov/openwrt-shaper/internal/store"
	"github.com/akazakov/openwrt-shaper/web"
)

var version = "dev"

func main() {
	configPath := flag.String("config", "/etc/slqm/config.json", "path to config file")
	dbPath := flag.String("db", "/var/lib/slqm/state.db", "path to state database")
	showVersion := flag.Bool("version", false, "print version and exit")
	flag.Parse()

	if *showVersion {
		fmt.Println("slqm", version)
		os.Exit(0)
	}

	log.SetFlags(log.Ldate | log.Ltime | log.Lmsgprefix)
	log.SetPrefix("[slqm] ")
	log.Printf("starting slqm %s", version)

	// Load configuration
	cfg, err := config.Load(*configPath)
	if err != nil {
		log.Fatalf("load config: %v", err)
	}
	cfg.SetFilePath(*configPath)
	log.Printf("config loaded from %s", *configPath)

	// Auto-detect interfaces if set to "auto"
	snap := cfg.Snapshot()
	if snap.WANIface == "auto" || snap.LANIface == "auto" {
		wan, lan := snap.WANIface, snap.LANIface
		if wan == "auto" {
			if detected, err := netctl.DetectWANIface(); err != nil {
				log.Printf("warning: WAN auto-detect failed: %v (falling back to eth0)", err)
				wan = "eth0"
			} else {
				wan = detected
			}
		}
		if lan == "auto" {
			if detected, err := netctl.DetectLANIface(wan); err != nil {
				log.Printf("warning: LAN auto-detect failed: %v (falling back to br-lan)", err)
				lan = "br-lan"
			} else {
				lan = detected
			}
		}
		cfg.ResolveIfaces(wan, lan)
		log.Printf("interfaces: wan=%s lan=%s", wan, lan)
	}

	// Ensure database directory exists
	if err := os.MkdirAll(filepath.Dir(*dbPath), 0755); err != nil {
		log.Fatalf("create db dir: %v", err)
	}

	// Open state database
	st, err := store.Open(*dbPath)
	if err != nil {
		log.Fatalf("open store: %v", err)
	}
	defer st.Close()

	// Create context with signal handling
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGTERM, syscall.SIGINT)

	// Create dish client
	snap = cfg.Snapshot()
	dishClient := dish.NewClient(snap.DishAddr, snap.WANIface)

	// Create engine
	eng := engine.New(cfg, st, dishClient)

	// Setup network (IFB, nftables, tc)
	if err := eng.Setup(); err != nil {
		log.Fatalf("engine setup: %v", err)
	}

	// Create API handler and WebSocket hub
	handler := api.NewHandler(eng)
	hub := api.NewHub(eng)

	// Setup HTTP routes
	mux := http.NewServeMux()
	webFS, err := fs.Sub(web.Assets, ".")
	if err != nil {
		log.Fatalf("web assets: %v", err)
	}
	api.SetupRoutes(mux, handler, hub, webFS)

	// Listen on both IPv4 and IPv6 with separate http.Server instances.
	// Go's dual-stack socket doesn't receive IPv4 on GL.iNet's kernel,
	// and calling Serve() twice on one server may cause Accept starvation.
	listenPort := netctl.ExtractPort(snap.ListenAddr)
	netctl.OpenFirewallPort(listenPort)

	server6 := &http.Server{
		Handler:      mux,
		ReadTimeout:  30 * time.Second,
		WriteTimeout: 30 * time.Second,
	}
	ln6, err := net.Listen("tcp6", "[::]:"+listenPort)
	if err != nil {
		log.Printf("warning: IPv6 listen failed: %v", err)
	}

	server4 := &http.Server{
		Handler:      mux,
		ReadTimeout:  30 * time.Second,
		WriteTimeout: 30 * time.Second,
	}
	ln4, err := net.Listen("tcp4", "0.0.0.0:"+listenPort)
	if err != nil {
		log.Printf("warning: IPv4 listen failed: %v", err)
	}

	// Start all goroutines
	if ln6 != nil {
		go func() {
			log.Printf("HTTP server listening on %s (IPv6)", ln6.Addr())
			if err := server6.Serve(ln6); err != http.ErrServerClosed {
				log.Printf("http server (IPv6): %v", err)
			}
		}()
	}
	if ln4 != nil {
		go func() {
			log.Printf("HTTP server listening on %s (IPv4)", ln4.Addr())
			if err := server4.Serve(ln4); err != http.ErrServerClosed {
				log.Printf("http server (IPv4): %v", err)
			}
		}()
	}

	// IPv4 diagnostic: test raw TCP accept on a random port
	go func() {
		time.Sleep(2 * time.Second)
		rawLn, err := net.Listen("tcp4", "127.0.0.1:0")
		if err != nil {
			log.Printf("IPv4 DIAG: raw listen failed: %v", err)
			return
		}
		rawAddr := rawLn.Addr().String()
		log.Printf("IPv4 DIAG: raw listener on %s", rawAddr)

		accepted := make(chan struct{})
		go func() {
			c, err := rawLn.Accept()
			if err != nil {
				log.Printf("IPv4 DIAG: raw accept failed: %v", err)
				return
			}
			log.Printf("IPv4 DIAG: raw accept OK from %s", c.RemoteAddr())
			c.Write([]byte("hello"))
			c.Close()
			close(accepted)
		}()

		time.Sleep(100 * time.Millisecond)
		c, err := net.DialTimeout("tcp4", rawAddr, 5*time.Second)
		if err != nil {
			log.Printf("IPv4 DIAG: raw dial to %s failed: %v", rawAddr, err)
		} else {
			buf := make([]byte, 16)
			n, _ := c.Read(buf)
			log.Printf("IPv4 DIAG: raw dial OK, got %q", string(buf[:n]))
			c.Close()
		}

		<-accepted
		rawLn.Close()

		// Now test the actual HTTP listener
		c2, err := net.DialTimeout("tcp4", "127.0.0.1:"+listenPort, 5*time.Second)
		if err != nil {
			log.Printf("IPv4 DIAG: HTTP dial to 127.0.0.1:%s failed: %v", listenPort, err)
		} else {
			log.Printf("IPv4 DIAG: HTTP dial to 127.0.0.1:%s OK", listenPort)
			c2.Close()
		}
	}()

	stopWS := make(chan struct{})
	go hub.Run(stopWS)

	engineDone := make(chan struct{})
	go func() {
		eng.Run(ctx)
		close(engineDone)
	}()

	// Start dish poller
	dishInterval := time.Duration(snap.DishPollIntervalSec) * time.Second
	go dishClient.RunPoller(ctx, dishInterval)

	// Wait for shutdown signal
	sig := <-sigCh
	log.Printf("received signal %v, shutting down", sig)

	// Cancel engine context — triggers engine.shutdown() which cleans up
	// tc, nftables, and IFB. Wait for it to complete before proceeding.
	cancel()

	// Wait for engine cleanup with a timeout (procd sends SIGKILL after ~5s)
	select {
	case <-engineDone:
		log.Println("engine cleanup complete")
	case <-time.After(3 * time.Second):
		log.Println("engine cleanup timed out")
	}

	// Stop WebSocket hub
	close(stopWS)

	// Shutdown HTTP servers
	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer shutdownCancel()
	server6.Shutdown(shutdownCtx)
	server4.Shutdown(shutdownCtx)

	// Close firewall port
	netctl.CloseFirewallPort(listenPort)

	log.Println("shutdown complete")
}
