package main

import (
	"context"
	"flag"
	"fmt"
	"io/fs"
	"log"
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

	// Start HTTP server
	server := &http.Server{
		Addr:         snap.ListenAddr,
		Handler:      mux,
		ReadTimeout:  30 * time.Second,
		WriteTimeout: 30 * time.Second,
	}

	// Start all goroutines
	go func() {
		log.Printf("HTTP server listening on %s", snap.ListenAddr)
		if err := server.ListenAndServe(); err != http.ErrServerClosed {
			log.Fatalf("http server: %v", err)
		}
	}()

	stopWS := make(chan struct{})
	go hub.Run(stopWS)

	go eng.Run(ctx)

	// Start dish poller
	dishInterval := time.Duration(snap.DishPollIntervalSec) * time.Second
	go dishClient.RunPoller(ctx, dishInterval)

	// Wait for shutdown signal
	sig := <-sigCh
	log.Printf("received signal %v, shutting down", sig)

	// Stop WebSocket hub
	close(stopWS)

	// Cancel engine context
	cancel()

	// Shutdown HTTP server
	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()
	server.Shutdown(shutdownCtx)

	log.Println("shutdown complete")
}
