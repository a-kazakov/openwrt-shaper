package api

import (
	"io/fs"
	"net/http"
)

// SetupRoutes configures all HTTP routes.
func SetupRoutes(mux *http.ServeMux, handler *Handler, hub *Hub, webFS fs.FS) {
	// API v1
	mux.HandleFunc("/api/v1/state", methodGuard("GET", handler.HandleState))
	mux.HandleFunc("/api/v1/config", func(w http.ResponseWriter, r *http.Request) {
		switch r.Method {
		case "GET":
			handler.HandleGetConfig(w, r)
		case "PUT":
			handler.HandleUpdateConfig(w, r)
		default:
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		}
	})
	mux.HandleFunc("/api/v1/sync", methodGuard("POST", handler.HandleSync))
	mux.HandleFunc("/api/v1/quota/adjust", methodGuard("POST", handler.HandleQuotaAdjust))
	mux.HandleFunc("/api/v1/quota/reset", methodGuard("POST", handler.HandleQuotaReset))
	mux.HandleFunc("/api/v1/history", methodGuard("GET", handler.HandleHistory))

	// Device-specific routes
	mux.HandleFunc("/api/v1/device/", func(w http.ResponseWriter, r *http.Request) {
		switch r.Method {
		case "POST":
			if containsSuffix(r.URL.Path, "/turbo") {
				handler.HandleDeviceTurbo(w, r)
			} else if containsSuffix(r.URL.Path, "/bucket") {
				handler.HandleSetBucket(w, r)
			} else {
				http.Error(w, "not found", http.StatusNotFound)
			}
		case "DELETE":
			if containsSuffix(r.URL.Path, "/turbo") {
				handler.HandleCancelTurbo(w, r)
			} else {
				http.Error(w, "not found", http.StatusNotFound)
			}
		default:
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		}
	})

	// WebSocket
	mux.HandleFunc("/ws", hub.HandleWS)

	// Static web UI
	mux.Handle("/", http.FileServer(http.FS(webFS)))
}

func methodGuard(method string, handler http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if r.Method != method {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		handler(w, r)
	}
}

func containsSuffix(path, suffix string) bool {
	return len(path) >= len(suffix) && path[len(path)-len(suffix):] == suffix
}
