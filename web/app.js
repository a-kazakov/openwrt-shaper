/* SLQM Dashboard — Vanilla JS, no dependencies */
/* global fetch, WebSocket, document, window, confirm */
"use strict";

(function () {
  // ── Helpers ────────────────────────────────────────────

  function formatBytes(b) {
    if (b == null) return "--";
    var abs = Math.abs(b);
    if (abs < 1024) return b + " B";
    if (abs < 1048576) return (b / 1024).toFixed(1) + " KB";
    if (abs < 1073741824) return (b / 1048576).toFixed(1) + " MB";
    return (b / 1073741824).toFixed(2) + " GB";
  }

  function formatRate(bps) {
    if (bps == null) return "--";
    var abs = Math.abs(bps);
    if (abs < 1000) return bps + " bps";
    if (abs < 1000000) return (bps / 1000).toFixed(1) + " kbps";
    if (abs < 1000000000) return (bps / 1000000).toFixed(1) + " Mbps";
    return (bps / 1000000000).toFixed(2) + " Gbps";
  }

  function formatRateKbit(kbit) {
    if (kbit == null) return "--";
    if (kbit < 1000) return kbit + " kbps";
    return (kbit / 1000).toFixed(1) + " Mbps";
  }

  function formatDuration(seconds) {
    if (seconds == null || seconds <= 0) return "--";
    var d = Math.floor(seconds / 86400);
    var h = Math.floor((seconds % 86400) / 3600);
    var m = Math.floor((seconds % 3600) / 60);
    if (d > 0) return d + "d " + h + "h";
    if (h > 0) return h + "h " + m + "m";
    return m + "m";
  }

  function $(id) {
    return document.getElementById(id);
  }

  // ── Toast Notifications ────────────────────────────────

  function toast(msg, type) {
    var el = document.createElement("div");
    el.className = "toast " + (type || "info");
    el.textContent = msg;
    $("toast-container").appendChild(el);
    setTimeout(function () {
      el.style.opacity = "0";
      el.style.transition = "opacity 0.3s";
      setTimeout(function () { el.remove(); }, 300);
    }, 3000);
  }

  // ── API helpers ────────────────────────────────────────

  function apiPost(url, body) {
    return fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body)
    }).then(function (r) {
      if (!r.ok) return r.json().then(function (e) { throw new Error(e.error || r.statusText); });
      return r.json();
    });
  }

  function apiPut(url, body) {
    return fetch(url, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body)
    }).then(function (r) {
      if (!r.ok) return r.json().then(function (e) { throw new Error(e.error || r.statusText); });
      return r.json();
    });
  }

  function apiDelete(url) {
    return fetch(url, { method: "DELETE" }).then(function (r) {
      if (!r.ok) return r.json().then(function (e) { throw new Error(e.error || r.statusText); });
      return r.json();
    });
  }

  function apiGet(url) {
    return fetch(url).then(function (r) {
      if (!r.ok) return r.json().then(function (e) { throw new Error(e.error || r.statusText); });
      return r.json();
    });
  }

  // ── State ──────────────────────────────────────────────

  var state = null;
  var configCache = null;

  // ── WebSocket ──────────────────────────────────────────

  var ws = null;
  var wsRetry = 1;
  var wsMaxRetry = 30;

  function wsConnect() {
    var proto = location.protocol === "https:" ? "wss:" : "ws:";
    var url = proto + "//" + location.host + "/ws";
    ws = new WebSocket(url);

    ws.onopen = function () {
      wsRetry = 1;
      $("ws-dot").classList.add("connected");
      $("ws-label").textContent = "Connected";
    };

    ws.onmessage = function (ev) {
      try {
        state = JSON.parse(ev.data);
        render();
      } catch (e) {
        console.error("ws parse:", e);
      }
    };

    ws.onclose = function () {
      $("ws-dot").classList.remove("connected");
      $("ws-label").textContent = "Reconnecting...";
      var delay = Math.min(wsRetry * 1000, wsMaxRetry * 1000);
      wsRetry = Math.min(wsRetry * 2, wsMaxRetry);
      setTimeout(wsConnect, delay);
    };

    ws.onerror = function () {
      ws.close();
    };
  }

  // ── Render ─────────────────────────────────────────────

  function render() {
    if (!state) return;
    renderStats();
    renderQuotaBar();
    renderCurveChart();
    renderDevices();
    renderSparkline();
  }

  // ── Stats Cards ────────────────────────────────────────

  function renderStats() {
    var q = state.quota;
    if (!q) return;

    $("stat-used").textContent = formatBytes(q.used);
    $("stat-used-sub").textContent = "Down: " + formatBytes(q.used_download) +
      " / Up: " + formatBytes(q.used_upload);

    $("stat-remaining").textContent = formatBytes(q.remaining);
    $("stat-remaining-sub").textContent = q.billing_month || "";

    var c = state.curve;
    if (c) {
      $("stat-rate").textContent = formatRateKbit(c.rate_kbit);
      $("stat-rate-sub").textContent = "shape=" + (c.shape || 0).toFixed(2) +
        " ratio=" + (c.down_up_ratio || 0).toFixed(2);
    }

    var tp = state.throughput;
    if (tp) {
      $("stat-throughput").textContent = formatRate(tp.current_down_bps);
      $("stat-throughput-sub").textContent = "Up: " + formatRate(tp.current_up_bps);
    }
  }

  // ── Quota Bar ──────────────────────────────────────────

  function renderQuotaBar() {
    var q = state.quota;
    if (!q) return;

    var pct = q.pct || 0;
    if (pct > 100) pct = 100;
    if (pct < 0) pct = 0;

    $("quota-pct").textContent = q.pct + "% used (" +
      formatBytes(q.used) + " / " + formatBytes(q.total) + ")";

    var fill = $("quota-fill");
    fill.style.width = pct + "%";
    fill.className = "quota-bar-fill";
    if (q.pct >= 90) {
      fill.classList.add("danger");
    } else if (q.pct >= 70) {
      fill.classList.add("warning");
    }
  }

  // ── Curve Chart ────────────────────────────────────────

  function renderCurveChart() {
    var c = state.curve;
    var q = state.quota;
    if (!c || !q) return;

    var svg = $("curve-svg");
    var w = 600, h = 160;
    var pad = { top: 15, right: 15, bottom: 25, left: 55 };
    var pw = w - pad.left - pad.right;
    var ph = h - pad.top - pad.bottom;

    var shape = c.shape || 0.4;
    var maxRate = c.rate_kbit ? c.rate_kbit * 2 : 50000;
    // Use max configured rate if available from config
    if (configCache && configCache.max_rate_kbit) {
      maxRate = configCache.max_rate_kbit;
    }
    var minRate = configCache ? (configCache.min_rate_kbit || 1000) : 1000;

    // Build curve points
    var points = [];
    var areaPoints = [];
    var steps = 100;
    for (var i = 0; i <= steps; i++) {
      var ratio = i / steps; // remaining ratio (1 = full, 0 = empty)
      var curved = Math.pow(ratio, shape);
      var rate = minRate + (maxRate - minRate) * curved;
      var x = pad.left + (1 - ratio) * pw; // X: 0=full remaining (left), 1=empty (right)
      var y = pad.top + ph - (rate / maxRate) * ph;
      points.push(x.toFixed(1) + "," + y.toFixed(1));
      areaPoints.push(x.toFixed(1) + "," + y.toFixed(1));
    }

    // Area: close path at bottom
    areaPoints.push((pad.left + pw).toFixed(1) + "," + (pad.top + ph).toFixed(1));
    areaPoints.push(pad.left.toFixed(1) + "," + (pad.top + ph).toFixed(1));

    // "You are here" dot
    var usedPct = q.total > 0 ? q.used / q.total : 0;
    if (usedPct > 1) usedPct = 1;
    var remainRatio = 1 - usedPct;
    var youCurved = Math.pow(remainRatio, shape);
    var youRate = minRate + (maxRate - minRate) * youCurved;
    var youX = pad.left + usedPct * pw;
    var youY = pad.top + ph - (youRate / maxRate) * ph;

    var html = "";

    // Grid lines
    for (var gi = 0; gi <= 4; gi++) {
      var gy = pad.top + (gi / 4) * ph;
      var gridRate = maxRate * (1 - gi / 4);
      html += '<line class="axis-line" x1="' + pad.left + '" y1="' + gy +
        '" x2="' + (w - pad.right) + '" y2="' + gy + '"/>';
      html += '<text class="axis-label" x="' + (pad.left - 5) + '" y="' +
        (gy + 3) + '" text-anchor="end">' + formatRateKbit(Math.round(gridRate)) + '</text>';
    }

    // X-axis labels
    html += '<text class="axis-label" x="' + pad.left + '" y="' + (h - 3) +
      '" text-anchor="start">0%</text>';
    html += '<text class="axis-label" x="' + (pad.left + pw / 2) + '" y="' +
      (h - 3) + '" text-anchor="middle">50% used</text>';
    html += '<text class="axis-label" x="' + (pad.left + pw) + '" y="' +
      (h - 3) + '" text-anchor="end">100%</text>';

    // Area
    html += '<polygon class="curve-area" points="' + areaPoints.join(" ") + '"/>';

    // Line
    html += '<polyline class="curve-line" points="' + points.join(" ") + '"/>';

    // "You are here" vertical dashed line
    html += '<line class="you-line" x1="' + youX.toFixed(1) + '" y1="' +
      pad.top + '" x2="' + youX.toFixed(1) + '" y2="' + (pad.top + ph) + '"/>';

    // Dot
    html += '<circle class="you-dot" cx="' + youX.toFixed(1) + '" cy="' +
      youY.toFixed(1) + '" r="5"/>';

    // Label
    var labelX = youX + 8;
    var labelAnchor = "start";
    if (youX > w - 100) { labelX = youX - 8; labelAnchor = "end"; }
    html += '<text class="you-label" x="' + labelX.toFixed(1) + '" y="' +
      (youY - 8).toFixed(1) + '" text-anchor="' + labelAnchor + '">' +
      formatRateKbit(Math.round(youRate)) + '</text>';

    svg.innerHTML = html;
  }

  // ── Device Table ───────────────────────────────────────

  function renderDevices() {
    var devices = state.devices;
    var tbody = $("device-tbody");

    if (!devices || devices.length === 0) {
      tbody.innerHTML = '<tr><td colspan="7" class="no-devices">No devices connected</td></tr>';
      return;
    }

    var rows = "";
    for (var i = 0; i < devices.length; i++) {
      var d = devices[i];
      var name = d.hostname || d.ip || d.mac;
      var modeClass = d.mode || "sustained";

      // Bucket bar
      var bucketPct = d.bucket_pct || 0;
      var bucketClass = "";
      if (bucketPct <= 10) bucketClass = "empty";
      else if (bucketPct <= 30) bucketClass = "low";

      // Turbo button
      var turboHtml;
      if (d.turbo) {
        var turboExpiry = "";
        if (d.turbo_expires) {
          var remaining = d.turbo_expires - Math.floor(Date.now() / 1000);
          if (remaining > 0) turboExpiry = " (" + formatDuration(remaining) + ")";
        }
        turboHtml = '<button class="turbo-btn active" data-mac="' + escHtml(d.mac) +
          '" data-action="cancel">Stop' + turboExpiry + '</button>';
      } else {
        turboHtml = '<button class="turbo-btn" data-mac="' + escHtml(d.mac) +
          '" data-action="start">Turbo</button>';
      }

      rows += "<tr>" +
        '<td><span class="hostname">' + escHtml(name) + "</span><br>" +
        '<span class="mac">' + escHtml(d.mac) + "</span></td>" +
        '<td><span class="mode-badge ' + modeClass + '">' + escHtml(d.mode) + "</span></td>" +
        "<td>" +
          '<span class="bucket-bar"><span class="bucket-bar-fill ' + bucketClass +
          '" style="width:' + bucketPct + '%"></span></span>' +
          '<span class="bucket-pct">' + bucketPct + "%</span>" +
        "</td>" +
        "<td>" + formatRate(d.rate_down_bps) + " / " + formatRate(d.rate_up_bps) + "</td>" +
        "<td>" + formatBytes(d.session_bytes) + "</td>" +
        "<td>" + formatBytes(d.cycle_bytes) + "</td>" +
        "<td>" + turboHtml + "</td>" +
        "</tr>";
    }

    tbody.innerHTML = rows;
  }

  function escHtml(s) {
    if (!s) return "";
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;")
      .replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }

  // ── Throughput Sparkline ───────────────────────────────

  function renderSparkline() {
    var tp = state.throughput;
    if (!tp) return;

    // Current rates header
    var ratesEl = $("sparkline-rates");
    ratesEl.innerHTML =
      '<span class="down">' + formatRate(tp.current_down_bps) + ' down</span>' +
      '<span class="sep">/</span>' +
      '<span class="up">' + formatRate(tp.current_up_bps) + ' up</span>';

    var samples = tp.samples_1m;
    if (!samples || samples.length < 2) return;

    var svg = $("sparkline-svg");
    var w = 600, h = 80;

    // Find max for scale
    var maxBps = 1000; // minimum 1 kbps scale
    for (var i = 0; i < samples.length; i++) {
      if (samples[i].down_bps > maxBps) maxBps = samples[i].down_bps;
      if (samples[i].up_bps > maxBps) maxBps = samples[i].up_bps;
    }
    maxBps *= 1.1; // 10% headroom

    var n = samples.length;
    var downPts = [];
    var upPts = [];
    var downArea = [];
    var upArea = [];

    for (var j = 0; j < n; j++) {
      var x = (j / (n - 1)) * w;
      var yDown = h - (samples[j].down_bps / maxBps) * h;
      var yUp = h - (samples[j].up_bps / maxBps) * h;
      var xStr = x.toFixed(1);
      downPts.push(xStr + "," + yDown.toFixed(1));
      upPts.push(xStr + "," + yUp.toFixed(1));
      downArea.push(xStr + "," + yDown.toFixed(1));
      upArea.push(xStr + "," + yUp.toFixed(1));
    }

    // Close area polygons
    downArea.push(w.toFixed(1) + "," + h);
    downArea.push("0," + h);
    upArea.push(w.toFixed(1) + "," + h);
    upArea.push("0," + h);

    var html = "";
    html += '<polygon class="area-down" points="' + downArea.join(" ") + '"/>';
    html += '<polygon class="area-up" points="' + upArea.join(" ") + '"/>';
    html += '<polyline class="line-down" points="' + downPts.join(" ") + '"/>';
    html += '<polyline class="line-up" points="' + upPts.join(" ") + '"/>';

    svg.innerHTML = html;
  }

  // ── Config Panel ───────────────────────────────────────

  var configFields = [
    { id: "cfg-monthly-quota-gb",         key: "monthly_quota_gb",          type: "int" },
    { id: "cfg-billing-reset-day",        key: "billing_reset_day",         type: "int" },
    { id: "cfg-plan-cost-monthly",        key: "plan_cost_monthly",         type: "float" },
    { id: "cfg-overage-cost-per-gb",      key: "overage_cost_per_gb",       type: "float" },
    { id: "cfg-max-rate-kbit",            key: "max_rate_kbit",             type: "int" },
    { id: "cfg-min-rate-kbit",            key: "min_rate_kbit",             type: "int" },
    { id: "cfg-curve-shape",              key: "curve_shape",               type: "float" },
    { id: "cfg-down-up-ratio",            key: "down_up_ratio",             type: "float" },
    { id: "cfg-bucket-duration-sec",      key: "bucket_duration_sec",       type: "int" },
    { id: "cfg-burst-drain-ratio",        key: "burst_drain_ratio",         type: "float" },
    { id: "cfg-wan-iface",                key: "wan_iface",                 type: "str" },
    { id: "cfg-lan-iface",                key: "lan_iface",                 type: "str" },
    { id: "cfg-ifb-iface",               key: "ifb_iface",                 type: "str" },
    { id: "cfg-dish-addr",                key: "dish_addr",                 type: "str" },
    { id: "cfg-tick-interval-sec",        key: "tick_interval_sec",         type: "int" },
    { id: "cfg-save-interval-sec",        key: "save_interval_sec",         type: "int" },
    { id: "cfg-device-scan-interval-sec", key: "device_scan_interval_sec",  type: "int" },
    { id: "cfg-dish-poll-interval-sec",   key: "dish_poll_interval_sec",    type: "int" }
  ];

  function openConfig() {
    $("config-overlay").classList.add("open");
    $("config-panel").classList.add("open");
    loadConfig();
  }

  function closeConfig() {
    $("config-overlay").classList.remove("open");
    $("config-panel").classList.remove("open");
  }

  function loadConfig() {
    apiGet("/api/v1/config").then(function (cfg) {
      configCache = cfg;
      for (var i = 0; i < configFields.length; i++) {
        var f = configFields[i];
        var el = $(f.id);
        if (el && cfg[f.key] !== undefined) {
          el.value = cfg[f.key];
        }
      }
    }).catch(function (e) {
      toast("Failed to load config: " + e.message, "error");
    });
  }

  function saveConfig() {
    var payload = {};
    for (var i = 0; i < configFields.length; i++) {
      var f = configFields[i];
      var el = $(f.id);
      if (!el) continue;
      var val = el.value;
      if (val === "") continue;
      if (f.type === "int") {
        payload[f.key] = parseInt(val, 10);
      } else if (f.type === "float") {
        payload[f.key] = parseFloat(val);
      } else {
        payload[f.key] = val;
      }
    }

    apiPut("/api/v1/config", payload).then(function (cfg) {
      configCache = cfg;
      toast("Configuration saved", "success");
      closeConfig();
    }).catch(function (e) {
      toast("Save failed: " + e.message, "error");
    });
  }

  // ── Sync Button ────────────────────────────────────────

  function handleSync() {
    var gbEl = $("sync-gb");
    var gb = parseFloat(gbEl.value);
    if (isNaN(gb) || gb < 0) {
      toast("Enter a valid usage value in GB", "error");
      return;
    }

    apiPost("/api/v1/sync", {
      starlink_used_gb: gb,
      source: "manual"
    }).then(function (resp) {
      if (resp.adjusted_by) {
        toast("Synced: adjusted by " + formatBytes(resp.adjusted_by), "success");
      } else {
        toast(resp.note || "Sync complete", "info");
      }
      gbEl.value = "";
    }).catch(function (e) {
      toast("Sync failed: " + e.message, "error");
    });
  }

  // ── Adjust Button ──────────────────────────────────────

  function handleAdjust() {
    var gbEl = $("adjust-gb");
    var gb = parseFloat(gbEl.value);
    if (isNaN(gb)) {
      toast("Enter a valid GB value", "error");
      return;
    }

    var deltaBytes = Math.round(gb * 1073741824);
    apiPost("/api/v1/quota/adjust", {
      delta_bytes: deltaBytes
    }).then(function () {
      toast("Adjusted by " + (gb > 0 ? "+" : "") + gb.toFixed(2) + " GB", "success");
      gbEl.value = "";
    }).catch(function (e) {
      toast("Adjust failed: " + e.message, "error");
    });
  }

  // ── Reset Button ───────────────────────────────────────

  function handleReset() {
    if (!confirm("Reset billing cycle? This will zero out all usage counters.")) return;

    apiPost("/api/v1/quota/reset", {}).then(function () {
      toast("Billing cycle reset", "success");
    }).catch(function (e) {
      toast("Reset failed: " + e.message, "error");
    });
  }

  // ── Turbo Toggle (event delegation) ────────────────────

  function handleTurboClick(ev) {
    var btn = ev.target.closest(".turbo-btn");
    if (!btn) return;

    var mac = btn.getAttribute("data-mac");
    var action = btn.getAttribute("data-action");

    if (action === "start") {
      apiPost("/api/v1/device/" + encodeURIComponent(mac) + "/turbo", {
        duration_min: 15
      }).then(function () {
        toast("Turbo enabled for " + mac, "success");
      }).catch(function (e) {
        toast("Turbo failed: " + e.message, "error");
      });
    } else {
      apiDelete("/api/v1/device/" + encodeURIComponent(mac) + "/turbo").then(function () {
        toast("Turbo cancelled for " + mac, "info");
      }).catch(function (e) {
        toast("Cancel turbo failed: " + e.message, "error");
      });
    }
  }

  // ── Init ───────────────────────────────────────────────

  function init() {
    // Config panel
    $("config-toggle").addEventListener("click", openConfig);
    $("config-close").addEventListener("click", closeConfig);
    $("config-overlay").addEventListener("click", closeConfig);
    $("config-save").addEventListener("click", saveConfig);
    $("config-cancel").addEventListener("click", closeConfig);

    // Keyboard: Escape closes config
    document.addEventListener("keydown", function (ev) {
      if (ev.key === "Escape") closeConfig();
    });

    // Action buttons
    $("sync-btn").addEventListener("click", handleSync);
    $("adjust-btn").addEventListener("click", handleAdjust);
    $("reset-btn").addEventListener("click", handleReset);

    // Turbo buttons (delegated)
    $("device-tbody").addEventListener("click", handleTurboClick);

    // Start WebSocket
    wsConnect();

    // Also fetch initial config for curve chart rendering
    apiGet("/api/v1/config").then(function (cfg) {
      configCache = cfg;
    }).catch(function () {
      // Silently ignore — will populate on config panel open
    });
  }

  // Start when DOM is ready
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
