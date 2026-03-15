import type { ConfigValues } from "./types";

async function request<T>(
  url: string,
  opts?: RequestInit,
): Promise<T> {
  const res = await fetch(url, opts);
  const body = await res.json();
  if (!res.ok) {
    throw new Error(body.error || res.statusText);
  }
  return body as T;
}

export function getConfig(): Promise<ConfigValues> {
  return request<ConfigValues>("/api/v1/config");
}

export function updateConfig(
  values: Partial<ConfigValues>,
): Promise<ConfigValues> {
  return request<ConfigValues>("/api/v1/config", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(values),
  });
}

export function syncUsage(
  gb: number,
): Promise<{ adjusted_by?: number; note?: string }> {
  return request("/api/v1/sync", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ starlink_used_gb: gb, source: "manual" }),
  });
}

export function adjustQuota(deltaBytes: number): Promise<unknown> {
  return request("/api/v1/quota/adjust", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ delta_bytes: deltaBytes }),
  });
}

export function resetCycle(): Promise<unknown> {
  return request("/api/v1/quota/reset", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({}),
  });
}

export function enableTurbo(
  mac: string,
  durationMin: number = 15,
): Promise<unknown> {
  return request(`/api/v1/device/${encodeURIComponent(mac)}/turbo`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ duration_min: durationMin }),
  });
}

export function cancelTurbo(mac: string): Promise<unknown> {
  return request(`/api/v1/device/${encodeURIComponent(mac)}/turbo`, {
    method: "DELETE",
  });
}
