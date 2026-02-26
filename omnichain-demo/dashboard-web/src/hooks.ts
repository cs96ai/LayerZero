import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  CrossChainMessage,
  GasInfo,
  LifecycleEvent,
  MetricsResponse,
  SubsystemHealth,
  SystemHealthResponse,
  TransactionDetailResponse,
  TransactionListResponse,
} from './types';

export type { CrossChainMessage, LifecycleEvent, MetricsResponse, TransactionDetailResponse };

// In production the relayer serves the dashboard on the same origin.
// In dev, fall back to localhost:3001.
const API_BASE = import.meta.env.VITE_RELAYER_HTTP || (typeof window !== 'undefined' && window.location.port !== '5173' ? '' : 'http://localhost:3001');
const WS_URL = import.meta.env.VITE_RELAYER_WS || (typeof window !== 'undefined' && window.location.port !== '5173'
  ? `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/ws`
  : 'ws://localhost:3001/ws');

// ──────────────────────────────────────────────
// Backend health / cold-start detection
// ──────────────────────────────────────────────

export function useBackendHealth() {
  const [status, setStatus] = useState<'checking' | 'online' | 'cold-start'>('checking');
  const [elapsed, setElapsed] = useState(0);
  const startRef = useRef(Date.now());

  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setInterval>;

    const check = async () => {
      try {
        const controller = new AbortController();
        const timeout = setTimeout(() => controller.abort(), 5000);
        const res = await fetch(`${API_BASE}/health`, { signal: controller.signal });
        clearTimeout(timeout);
        if (res.ok && active) {
          setStatus('online');
          clearInterval(timer);
        }
      } catch {
        if (active) {
          const secs = Math.floor((Date.now() - startRef.current) / 1000);
          setElapsed(secs);
          if (secs >= 5) setStatus('cold-start');
        }
      }
    };

    check();
    timer = setInterval(check, 3000);
    return () => { active = false; clearInterval(timer); };
  }, []);

  return { status, elapsed };
}

// ──────────────────────────────────────────────
// System health hook (subsystem status + gas)
// ──────────────────────────────────────────────

export function useSystemHealth(pollMs = 5000) {
  const [systems, setSystems] = useState<SubsystemHealth[]>([]);
  const [gas, setGas] = useState<GasInfo>({
    relayer_balance_wei: '0',
    relayer_balance_eth: '0.000000',
    gas_price_gwei: 0,
    estimated_txs_remaining: 0,
    is_low: false,
  });

  useEffect(() => {
    let active = true;
    const poll = async () => {
      try {
        const res = await fetch(`${API_BASE}/health/systems`);
        if (res.ok) {
          const data: SystemHealthResponse = await res.json();
          if (active) {
            setSystems(data.systems);
            setGas(data.gas);
          }
        }
      } catch { /* ignore */ }
    };
    poll();
    const id = setInterval(poll, pollMs);
    return () => { active = false; clearInterval(id); };
  }, [pollMs]);

  return { systems, gas };
}

// ──────────────────────────────────────────────
// REST hooks
// ──────────────────────────────────────────────

export function useTransactions(pollMs = 3000) {
  const [transactions, setTransactions] = useState<CrossChainMessage[]>([]);
  const [total, setTotal] = useState(0);

  useEffect(() => {
    let active = true;
    const poll = async () => {
      try {
        const res = await fetch(`${API_BASE}/transactions`);
        if (res.ok) {
          const data: TransactionListResponse = await res.json();
          if (active) {
            setTransactions(data.transactions);
            setTotal(data.total);
          }
        }
      } catch {
        // Relayer may not be running yet
      }
    };
    poll();
    const id = setInterval(poll, pollMs);
    return () => { active = false; clearInterval(id); };
  }, [pollMs]);

  return { transactions, total };
}

export function useTransactionDetail(nonce: number | null) {
  const [detail, setDetail] = useState<TransactionDetailResponse | null>(null);

  useEffect(() => {
    if (nonce === null) { setDetail(null); return; }
    let active = true;
    const load = async () => {
      try {
        const res = await fetch(`${API_BASE}/transactions/${nonce}`);
        if (res.ok) {
          const data: TransactionDetailResponse = await res.json();
          if (active) setDetail(data);
        }
      } catch { /* ignore */ }
    };
    load();
    const id = setInterval(load, 2000);
    return () => { active = false; clearInterval(id); };
  }, [nonce]);

  return detail;
}

export function useMetrics(pollMs = 3000) {
  const [metrics, setMetrics] = useState<MetricsResponse>({
    total_transactions: 0, settled: 0, failed: 0, pending: 0,
    total_retries: 0,
  });

  useEffect(() => {
    let active = true;
    const poll = async () => {
      try {
        const res = await fetch(`${API_BASE}/metrics`);
        if (res.ok) {
          const data: MetricsResponse = await res.json();
          if (active) setMetrics(data);
        }
      } catch { /* ignore */ }
    };
    poll();
    const id = setInterval(poll, pollMs);
    return () => { active = false; clearInterval(id); };
  }, [pollMs]);

  return metrics;
}

// ──────────────────────────────────────────────
// WebSocket hook for real-time events
// ──────────────────────────────────────────────

export function useEventStream() {
  const [events, setEvents] = useState<LifecycleEvent[]>([]);
  const [connected, setConnected] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    let reconnectTimer: ReturnType<typeof setTimeout>;

    const connect = () => {
      const ws = new WebSocket(WS_URL);
      wsRef.current = ws;

      ws.onopen = () => setConnected(true);
      ws.onclose = () => {
        setConnected(false);
        reconnectTimer = setTimeout(connect, 3000);
      };
      ws.onerror = () => ws.close();
      ws.onmessage = (msg) => {
        try {
          const event: LifecycleEvent = JSON.parse(msg.data);
          setEvents((prev) => {
            const next = [event, ...prev];
            return next.length > 500 ? next.slice(0, 500) : next;
          });
        } catch { /* ignore non-JSON */ }
      };
    };

    connect();
    return () => {
      clearTimeout(reconnectTimer);
      wsRef.current?.close();
    };
  }, []);

  const clearEvents = useCallback(() => setEvents([]), []);

  return { events, connected, clearEvents };
}

// ──────────────────────────────────────────────
// Simulation control hooks
// ──────────────────────────────────────────────

export function useSimulation() {
  const [running, setRunning] = useState(false);
  const [remainingSeconds, setRemainingSeconds] = useState(0);

  // Poll simulation status
  useEffect(() => {
    let active = true;
    const poll = async () => {
      try {
        const res = await fetch(`${API_BASE}/control/simulation-status`);
        if (res.ok) {
          const data = await res.json();
          if (active) {
            setRunning(data.running);
            setRemainingSeconds(data.remaining_seconds);
          }
        }
      } catch { /* ignore */ }
    };
    poll();
    const id = setInterval(poll, 2000);
    return () => { active = false; clearInterval(id); };
  }, []);

  const startSimulation = useCallback(async (durationMinutes = 60) => {
    try {
      await fetch(`${API_BASE}/control/start-simulation`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ duration_minutes: durationMinutes }),
      });
      setRunning(true);
    } catch { /* ignore */ }
  }, []);

  const stopSimulation = useCallback(async () => {
    try {
      await fetch(`${API_BASE}/control/stop-simulation`, { method: 'POST' });
      setRunning(false);
    } catch { /* ignore */ }
  }, []);

  const clearData = useCallback(async () => {
    try {
      await fetch(`${API_BASE}/control/clear-data`, { method: 'POST' });
      setRunning(false);
    } catch { /* ignore */ }
  }, []);

  return { running, remainingSeconds, startSimulation, stopSimulation, clearData };
}

// ──────────────────────────────────────────────
// AI Analysis hook
// ──────────────────────────────────────────────

export function useAnalysis() {
  const [analysis, setAnalysis] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [analyzedNonce, setAnalyzedNonce] = useState<number | null>(null);

  const analyze = useCallback(async (nonce: number) => {
    setLoading(true);
    setError(null);
    setAnalysis(null);
    setAnalyzedNonce(nonce);
    try {
      const res = await fetch(`${API_BASE}/analyze/${nonce}`, { method: 'POST' });
      if (!res.ok) {
        const text = await res.text();
        throw new Error(res.status === 503 ? 'OPENAI_API_KEY not configured on server' : `Analysis failed (${res.status}): ${text}`);
      }
      const data = await res.json();
      setAnalysis(data.analysis);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Unknown error');
    } finally {
      setLoading(false);
    }
  }, []);

  const clear = useCallback(() => {
    setAnalysis(null);
    setError(null);
    setAnalyzedNonce(null);
  }, []);

  return { analysis, loading, error, analyzedNonce, analyze, clear };
}
