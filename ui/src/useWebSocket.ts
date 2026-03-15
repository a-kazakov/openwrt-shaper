import { useEffect, useRef, useState, useCallback } from "react";
import type { StateSnapshot } from "./types";

export function useWebSocket() {
  const [state, setState] = useState<StateSnapshot | null>(null);
  const [connected, setConnected] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const retryRef = useRef(1);
  const mountedRef = useRef(true);

  const connect = useCallback(() => {
    if (!mountedRef.current) return;

    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    const url = `${proto}//${location.host}/ws`;
    const ws = new WebSocket(url);
    wsRef.current = ws;

    ws.onopen = () => {
      retryRef.current = 1;
      setConnected(true);
    };

    ws.onmessage = (ev) => {
      try {
        const snap: StateSnapshot = JSON.parse(ev.data);
        setState(snap);
      } catch {
        // ignore parse errors
      }
    };

    ws.onclose = () => {
      setConnected(false);
      if (!mountedRef.current) return;
      const delay = Math.min(retryRef.current * 1000, 30000);
      retryRef.current = Math.min(retryRef.current * 2, 30);
      setTimeout(connect, delay);
    };

    ws.onerror = () => {
      ws.close();
    };
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    connect();
    return () => {
      mountedRef.current = false;
      wsRef.current?.close();
    };
  }, [connect]);

  return { state, connected };
}
