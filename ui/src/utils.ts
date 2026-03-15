export function formatBytes(b: number | null | undefined): string {
  if (b == null) return "--";
  const abs = Math.abs(b);
  if (abs < 1024) return b + " B";
  if (abs < 1048576) return (b / 1024).toFixed(1) + " KB";
  if (abs < 1073741824) return (b / 1048576).toFixed(1) + " MB";
  return (b / 1073741824).toFixed(2) + " GB";
}

export function formatRate(bps: number | null | undefined): string {
  if (bps == null) return "--";
  const abs = Math.abs(bps);
  if (abs < 1000) return bps + " bps";
  if (abs < 1000000) return (bps / 1000).toFixed(1) + " kbps";
  if (abs < 1000000000) return (bps / 1000000).toFixed(1) + " Mbps";
  return (bps / 1000000000).toFixed(2) + " Gbps";
}

export function formatRateKbit(kbit: number | null | undefined): string {
  if (kbit == null) return "--";
  if (kbit < 1000) return kbit + " kbps";
  return (kbit / 1000).toFixed(1) + " Mbps";
}

export function formatMB(bytes: number): string {
  const mb = Math.round(bytes / 1048576);
  return mb + " MB";
}

/** Bytes rounded to nearest unit, no decimals, starting at KB. */
export function formatBytesRound(b: number | null | undefined): string {
  if (b == null) return "--";
  const abs = Math.abs(b);
  if (abs < 1048576) return Math.round(b / 1024) + " KB";
  if (abs < 1073741824) return Math.round(b / 1048576) + " MB";
  return Math.round(b / 1073741824) + " GB";
}

/** Bits/sec rounded to nearest unit, no decimals, starting at Kbps. */
export function formatRateRound(bps: number | null | undefined): string {
  if (bps == null) return "--";
  const abs = Math.abs(bps);
  if (abs < 1000000) return Math.round(bps / 1000) + " Kb/s";
  if (abs < 10000000) return (bps / 1000000).toFixed(1) + " Mb/s";
  if (abs < 1000000000) return Math.round(bps / 1000000) + " Mb/s";
  return Math.round(bps / 1000000000) + " Gb/s";
}

/** Format down/up bps pair into a compact string with shared unit: "▼4.0 / ▲1.0 Mb/s" */
export function formatLimitPair(downBps: number, upBps: number): string {
  const maxVal = Math.max(downBps, upBps);
  let unit: string;
  let div: number;
  if (maxVal >= 1000000000) {
    unit = "Gb/s";
    div = 1000000000;
  } else if (maxVal >= 1000000) {
    unit = "Mb/s";
    div = 1000000;
  } else {
    unit = "Kb/s";
    div = 1000;
  }
  const fmt = (v: number) => {
    const n = v / div;
    return n < 10 ? n.toFixed(1) : String(Math.round(n));
  };
  return `\u{25BC}${fmt(downBps)} / \u{25B2}${fmt(upBps)} ${unit}`;
}

// UI uses "throttled" instead of backend's "sustained" for user clarity
export function modeLabel(mode: string): string {
  return mode === "sustained" ? "throttled" : mode;
}

export function modeColor(mode: string): string {
  switch (mode) {
    case "burst":
      return "#60a5fa";
    case "sustained":
      return "#fbbf24";
    case "turbo":
      return "#4ade80";
    default:
      return "#666";
  }
}

export function formatDuration(seconds: number | null | undefined): string {
  if (seconds == null || seconds <= 0) return "--";
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (d > 0) return d + "d " + h + "h";
  if (h > 0) return h + "h " + m + "m";
  return m + "m";
}
