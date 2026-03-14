package api

import (
	"encoding/json"
	"log"
	"net/http"
	"sync"
	"time"

	"github.com/gorilla/websocket"
)

var upgrader = websocket.Upgrader{
	CheckOrigin: func(r *http.Request) bool { return true },
}

// Hub manages WebSocket connections and broadcasts state updates.
type Hub struct {
	mu      sync.RWMutex
	clients map[*websocket.Conn]struct{}
	engine  Engine
}

// NewHub creates a new WebSocket hub.
func NewHub(engine Engine) *Hub {
	return &Hub{
		clients: make(map[*websocket.Conn]struct{}),
		engine:  engine,
	}
}

// HandleWS upgrades the HTTP connection to WebSocket.
func (hub *Hub) HandleWS(w http.ResponseWriter, r *http.Request) {
	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		log.Printf("ws upgrade: %v", err)
		return
	}

	hub.mu.Lock()
	hub.clients[conn] = struct{}{}
	hub.mu.Unlock()

	// Send initial state
	if data, err := json.Marshal(hub.engine.Snapshot()); err == nil {
		conn.WriteMessage(websocket.TextMessage, data)
	}

	// Read loop (handle client disconnect)
	go func() {
		defer func() {
			hub.mu.Lock()
			delete(hub.clients, conn)
			hub.mu.Unlock()
			conn.Close()
		}()
		for {
			if _, _, err := conn.ReadMessage(); err != nil {
				return
			}
		}
	}()
}

// Broadcast sends the current state to all connected clients.
func (hub *Hub) Broadcast() {
	data, err := json.Marshal(hub.engine.Snapshot())
	if err != nil {
		log.Printf("ws broadcast marshal: %v", err)
		return
	}

	hub.mu.RLock()
	defer hub.mu.RUnlock()

	for conn := range hub.clients {
		conn.SetWriteDeadline(time.Now().Add(5 * time.Second))
		if err := conn.WriteMessage(websocket.TextMessage, data); err != nil {
			log.Printf("ws write: %v", err)
			go func(c *websocket.Conn) {
				hub.mu.Lock()
				delete(hub.clients, c)
				hub.mu.Unlock()
				c.Close()
			}(conn)
		}
	}
}

// Run starts the broadcast loop, pushing state every second.
func (hub *Hub) Run(stop <-chan struct{}) {
	ticker := time.NewTicker(1 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-stop:
			hub.mu.RLock()
			for conn := range hub.clients {
				conn.Close()
			}
			hub.mu.RUnlock()
			return
		case <-ticker.C:
			hub.Broadcast()
		}
	}
}
