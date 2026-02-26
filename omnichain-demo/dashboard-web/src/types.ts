export interface LifecycleEvent {
  trace_id: string;
  nonce: number;
  actor: 'ethereum' | 'relayer' | 'solana' | 'dashboard';
  step: 'locked' | 'observed' | 'verified' | 'executed' | 'minted' | 'burned' | 'rollback' | 'settled';
  status: 'success' | 'failure' | 'retry';
  timestamp: string;
  detail?: string;
}

export interface CrossChainMessage {
  id: number;
  nonce: number;
  trace_id: string;
  sender: string;
  amount: string;
  payload: string;
  deadline: number;
  description: string | null;
  state: string;
  result: string | null;
  solana_signature: string | null;
  eth_settle_tx: string | null;
  retry_count: number;
  error_message: string | null;
  created_at: string;
  updated_at: string;
}

export interface TransactionListResponse {
  transactions: CrossChainMessage[];
  total: number;
}

export interface TransactionDetailResponse {
  transaction: CrossChainMessage;
  events: LifecycleEvent[];
  proof: ProofBundle | null;
}

export interface ProofBundle {
  block_header: string;
  event_root: string;
  inclusion_proof: string[];
  validator_signature: string;
  relayer_address: string;
  nonce: number;
  verified: boolean;
}

export interface MetricsResponse {
  total_transactions: number;
  settled: number;
  failed: number;
  pending: number;
  total_retries: number;
}

// ──────────────────────────────────────────────
// System health types
// ──────────────────────────────────────────────

export type SubsystemStatus = 'online' | 'warming_up' | 'offline' | 'shutting_down';

export interface SubsystemHealth {
  name: string;
  status: SubsystemStatus;
  latency_ms: number | null;
  detail: string | null;
}

export interface GasInfo {
  relayer_balance_wei: string;
  relayer_balance_eth: string;
  gas_price_gwei: number;
  estimated_txs_remaining: number;
  is_low: boolean;
}

export interface SystemHealthResponse {
  systems: SubsystemHealth[];
  gas: GasInfo;
}

export const PIPELINE_STEPS: LifecycleEvent['step'][] = [
  'locked', 'observed', 'verified', 'executed', 'minted', 'burned', 'rollback', 'settled',
];

export const ACTOR_COLORS: Record<LifecycleEvent['actor'], string> = {
  ethereum: '#627EEA',
  relayer: '#14F195',
  solana: '#9945FF',
  dashboard: '#64748b',
};

export const STEP_ACTORS: Record<LifecycleEvent['step'], LifecycleEvent['actor']> = {
  locked: 'ethereum',
  observed: 'relayer',
  verified: 'relayer',
  executed: 'solana',
  minted: 'solana',
  burned: 'solana',
  rollback: 'relayer',
  settled: 'ethereum',
};

export const STATUS_COLORS: Record<LifecycleEvent['status'], string> = {
  success: '#22c55e',
  failure: '#ef4444',
  retry: '#f59e0b',
};
