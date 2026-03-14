package api

import (
	"encoding/json"
	"log"
	"net/http"
	"strings"
	"time"
)

// Engine is the interface the API needs from the quota engine.
type Engine interface {
	Snapshot() interface{}
	MonthUsed() int64
	AdjustQuota(delta int64)
	SetQuota(total int64)
	ResetBillingCycle()
	SetDeviceTurbo(mac string, duration time.Duration) error
	CancelDeviceTurbo(mac string) error
	SetDeviceBucket(mac string, tokensMB int64) error
	Config() interface{}
	UpdateConfig(data []byte) error
}

// Handler holds the API handlers.
type Handler struct {
	engine Engine
}

// NewHandler creates a new API handler.
func NewHandler(engine Engine) *Handler {
	return &Handler{engine: engine}
}

// HandleState returns the full current state snapshot.
func (h *Handler) HandleState(w http.ResponseWriter, r *http.Request) {
	respondJSON(w, http.StatusOK, h.engine.Snapshot())
}

// HandleGetConfig returns the current configuration.
func (h *Handler) HandleGetConfig(w http.ResponseWriter, r *http.Request) {
	respondJSON(w, http.StatusOK, h.engine.Config())
}

// HandleUpdateConfig applies a partial config update.
func (h *Handler) HandleUpdateConfig(w http.ResponseWriter, r *http.Request) {
	data, err := readBody(r)
	if err != nil {
		respondError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	if err := h.engine.UpdateConfig(data); err != nil {
		respondError(w, http.StatusBadRequest, err.Error())
		return
	}
	respondJSON(w, http.StatusOK, h.engine.Config())
}

// HandleSync handles manual quota sync with Starlink app.
func (h *Handler) HandleSync(w http.ResponseWriter, r *http.Request) {
	var req struct {
		StarlinkUsedGB float64 `json:"starlink_used_gb"`
		Source         string  `json:"source"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		respondError(w, http.StatusBadRequest, "invalid request")
		return
	}

	starlinkBytes := int64(req.StarlinkUsedGB * 1073741824)
	currentBytes := h.engine.MonthUsed()
	delta := starlinkBytes - currentBytes

	if delta > 0 {
		h.engine.AdjustQuota(delta)
		respondJSON(w, http.StatusOK, map[string]interface{}{
			"adjusted_by": delta,
			"new_total":   starlinkBytes,
			"source":      req.Source,
		})
	} else if delta < 0 {
		respondJSON(w, http.StatusOK, map[string]interface{}{
			"note":           "Router shows more than Starlink. No adjustment.",
			"router_bytes":   currentBytes,
			"starlink_bytes": starlinkBytes,
		})
	} else {
		respondJSON(w, http.StatusOK, map[string]interface{}{"note": "Already in sync"})
	}
}

// HandleQuotaAdjust handles manual quota adjustments.
func (h *Handler) HandleQuotaAdjust(w http.ResponseWriter, r *http.Request) {
	var req struct {
		DeltaBytes *int64 `json:"delta_bytes,omitempty"`
		SetBytes   *int64 `json:"set_bytes,omitempty"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		respondError(w, http.StatusBadRequest, "invalid request")
		return
	}

	if req.SetBytes != nil {
		h.engine.SetQuota(*req.SetBytes)
	} else if req.DeltaBytes != nil {
		h.engine.AdjustQuota(*req.DeltaBytes)
	} else {
		respondError(w, http.StatusBadRequest, "provide delta_bytes or set_bytes")
		return
	}

	respondJSON(w, http.StatusOK, h.engine.Snapshot())
}

// HandleQuotaReset resets the billing cycle.
func (h *Handler) HandleQuotaReset(w http.ResponseWriter, r *http.Request) {
	h.engine.ResetBillingCycle()
	respondJSON(w, http.StatusOK, h.engine.Snapshot())
}

// HandleDeviceTurbo enables turbo mode for a device.
func (h *Handler) HandleDeviceTurbo(w http.ResponseWriter, r *http.Request) {
	mac := extractMAC(r.URL.Path)
	if mac == "" {
		respondError(w, http.StatusBadRequest, "invalid MAC address")
		return
	}

	var req struct {
		DurationMin int `json:"duration_min"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		respondError(w, http.StatusBadRequest, "invalid request")
		return
	}
	if req.DurationMin <= 0 {
		req.DurationMin = 15
	}
	if req.DurationMin > 60 {
		req.DurationMin = 60
	}

	if err := h.engine.SetDeviceTurbo(mac, time.Duration(req.DurationMin)*time.Minute); err != nil {
		respondError(w, http.StatusNotFound, err.Error())
		return
	}

	respondJSON(w, http.StatusOK, h.engine.Snapshot())
}

// HandleCancelTurbo cancels turbo mode for a device.
func (h *Handler) HandleCancelTurbo(w http.ResponseWriter, r *http.Request) {
	mac := extractMAC(r.URL.Path)
	if mac == "" {
		respondError(w, http.StatusBadRequest, "invalid MAC address")
		return
	}

	if err := h.engine.CancelDeviceTurbo(mac); err != nil {
		respondError(w, http.StatusNotFound, err.Error())
		return
	}

	respondJSON(w, http.StatusOK, h.engine.Snapshot())
}

// HandleSetBucket sets a device's bucket tokens.
func (h *Handler) HandleSetBucket(w http.ResponseWriter, r *http.Request) {
	mac := extractMAC(r.URL.Path)
	if mac == "" {
		respondError(w, http.StatusBadRequest, "invalid MAC address")
		return
	}

	var req struct {
		TokensMB int64 `json:"tokens_mb"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		respondError(w, http.StatusBadRequest, "invalid request")
		return
	}

	if err := h.engine.SetDeviceBucket(mac, req.TokensMB); err != nil {
		respondError(w, http.StatusNotFound, err.Error())
		return
	}

	respondJSON(w, http.StatusOK, h.engine.Snapshot())
}

// HandleHistory returns historical usage data.
func (h *Handler) HandleHistory(w http.ResponseWriter, r *http.Request) {
	// TODO: implement with store history query
	respondJSON(w, http.StatusOK, map[string]interface{}{"samples": []interface{}{}})
}

func extractMAC(path string) string {
	// Path format: /api/v1/device/{mac}/turbo or /api/v1/device/{mac}/bucket
	parts := strings.Split(strings.Trim(path, "/"), "/")
	for i, p := range parts {
		if p == "device" && i+1 < len(parts) {
			mac := parts[i+1]
			// Basic MAC validation
			if len(mac) == 17 && strings.Count(mac, ":") == 5 {
				return strings.ToLower(mac)
			}
		}
	}
	return ""
}

func respondJSON(w http.ResponseWriter, status int, v interface{}) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	if err := json.NewEncoder(w).Encode(v); err != nil {
		log.Printf("api: encode response: %v", err)
	}
}

func respondError(w http.ResponseWriter, status int, msg string) {
	respondJSON(w, status, map[string]string{"error": msg})
}

func readBody(r *http.Request) ([]byte, error) {
	defer r.Body.Close()
	buf := make([]byte, 1024*64)
	n, err := r.Body.Read(buf)
	if err != nil && err.Error() != "EOF" {
		return nil, err
	}
	return buf[:n], nil
}
