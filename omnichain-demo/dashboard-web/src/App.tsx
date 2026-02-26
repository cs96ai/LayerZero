import { useState } from 'react';
import ReactMarkdown from 'react-markdown';
import { useAnalysis, useBackendHealth, useEventStream, useMetrics, useSimulation, useTransactionDetail, useTransactions } from './hooks';
import type { CrossChainMessage, LifecycleEvent } from './types';
import { ACTOR_COLORS, PIPELINE_STEPS, STATUS_COLORS, STEP_ACTORS } from './types';

// ──────────────────────────────────────────────
// Cold-start loading screen
// ──────────────────────────────────────────────

function ColdStartScreen({ elapsed }: { elapsed: number }) {
  return (
    <div className="min-h-screen flex flex-col items-center justify-center bg-gray-950 text-gray-100 px-6">
      <div className="max-w-lg text-center">
        <div className="mb-8">
          <svg className="w-20 h-20 mx-auto text-blue-500 animate-pulse" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M13 10V3L4 14h7v7l9-11h-7z" />
          </svg>
        </div>
        <h1 className="text-2xl font-bold mb-3">Omnichain Escrow Demo</h1>
        <p className="text-gray-300 mb-6">
          Cross-chain coordination between Ethereum and Solana with real-time lifecycle visualization, ECDSA-signed proof bundles, and failure simulation.
        </p>
        <div className="bg-gray-900 border border-gray-800 rounded-lg p-5 mb-6">
          <div className="flex items-center gap-3 mb-3">
            <div className="w-3 h-3 rounded-full bg-yellow-500 animate-pulse" />
            <span className="text-sm font-medium text-yellow-400">Warming up from cold start...</span>
          </div>
          <p className="text-xs text-gray-400 leading-relaxed">
            This demo is hosted on Azure free tier. The backend container scales to zero when idle
            and takes ~30-60 seconds to spin up on first visit. Hang tight — it's booting Anvil
            (local Ethereum node) and the Rust relayer right now.
          </p>
          <div className="mt-3 text-xs text-gray-500 font-mono">
            Waiting {elapsed}s...
          </div>
          <div className="mt-3 w-full bg-gray-800 rounded-full h-1.5 overflow-hidden">
            <div
              className="h-full bg-blue-600 rounded-full transition-all duration-1000"
              style={{ width: `${Math.min(100, (elapsed / 60) * 100)}%` }}
            />
          </div>
        </div>
        <div className="text-xs text-gray-600">
          Built with Rust (Tokio, Axum, ethers-rs) + React + Solidity + Solana BPF
        </div>
      </div>
    </div>
  );
}

// ──────────────────────────────────────────────
// Main App
// ──────────────────────────────────────────────

export default function App() {
  const backend = useBackendHealth();
  const { transactions } = useTransactions();
  const metrics = useMetrics();
  const { events, connected } = useEventStream();
  const simulation = useSimulation();
  const analysisHook = useAnalysis();
  const [selectedNonce, setSelectedNonce] = useState<number | null>(null);
  const [mobileTab, setMobileTab] = useState<'list' | 'detail' | 'events'>('list');
  const detail = useTransactionDetail(selectedNonce);

  if (backend.status !== 'online') {
    return <ColdStartScreen elapsed={backend.elapsed} />;
  }

  const selectTx = (nonce: number) => {
    setSelectedNonce(nonce);
    setMobileTab('detail');
    analysisHook.clear();
  };

  return (
    <div className="min-h-screen flex flex-col">
      {/* Top bar — global controls */}
      <header className="border-b border-gray-800 bg-gray-900/80 backdrop-blur px-3 sm:px-6 py-2 sm:py-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2 sm:gap-3 min-w-0">
            <div className="w-2.5 h-2.5 sm:w-3 sm:h-3 rounded-full flex-shrink-0" style={{ background: connected ? '#22c55e' : '#ef4444' }} />
            <h1 className="text-sm sm:text-lg font-bold tracking-tight truncate">Omnichain Escrow</h1>
            <a href="https://github.com/cs96ai/LayerZero" target="_blank" rel="noopener noreferrer" className="text-xs text-blue-400 hover:text-blue-300 underline underline-offset-2 hidden sm:inline">
              Source
            </a>
          </div>
          <div className="flex items-center gap-2 sm:gap-4">
            <div className="hidden md:flex">
              <MetricsBar metrics={metrics} />
            </div>
            <SimulationControls {...simulation} />
          </div>
        </div>
        {/* Mobile metrics row */}
        <div className="flex md:hidden mt-2 justify-center">
          <MetricsBar metrics={metrics} />
        </div>
      </header>

      {/* Mobile tab bar */}
      <div className="flex md:hidden border-b border-gray-800 bg-gray-900/60">
        {(['list', 'detail', 'events'] as const).map((tab) => (
          <button
            key={tab}
            onClick={() => setMobileTab(tab)}
            className={`flex-1 py-2 text-xs font-medium uppercase tracking-wider transition ${
              mobileTab === tab ? 'text-blue-400 border-b-2 border-blue-400' : 'text-gray-500'
            }`}
          >
            {tab === 'list' ? `Txns (${transactions.length})` : tab === 'detail' ? 'Detail' : 'Events'}
          </button>
        ))}
      </div>

      {/* Three-panel layout (desktop) / tabbed (mobile) */}
      <div className="flex flex-1 overflow-hidden">
        {/* Left — transaction list */}
        <aside className={`w-full md:w-72 border-r border-gray-800 overflow-y-auto bg-gray-900/50 ${mobileTab !== 'list' ? 'hidden md:block' : ''}`}>
          <div className="p-3 border-b border-gray-800 text-xs font-semibold text-gray-400 uppercase tracking-wider hidden md:block">
            Transactions ({transactions.length})
          </div>
          {transactions.length === 0 ? (
            <div className="p-4 text-sm text-gray-500">No transactions yet. Click <strong>Start</strong> above to begin the simulation.</div>
          ) : (
            transactions.map((tx) => (
              <TransactionRow
                key={tx.nonce}
                tx={tx}
                selected={tx.nonce === selectedNonce}
                onClick={() => selectTx(tx.nonce)}
              />
            ))
          )}
        </aside>

        {/* Center — lifecycle pipeline + analysis */}
        <main className={`flex-1 overflow-y-auto ${mobileTab !== 'detail' ? 'hidden md:block' : ''}`}>
          {selectedNonce !== null && detail ? (
            <div className="flex flex-col lg:flex-row h-full">
              {/* Timeline side */}
              <div className={`overflow-y-auto p-4 sm:p-6 ${analysisHook.analysis || analysisHook.loading ? 'lg:w-1/2 lg:border-r lg:border-gray-800' : 'w-full'}`}>
                <h2 className="text-lg sm:text-xl font-bold mb-1">
                  Transaction #{detail.transaction.nonce}
                </h2>
                {detail.transaction.description && (
                  <p className="text-sm text-gray-300 mb-1">{detail.transaction.description}</p>
                )}
                <p className="text-xs text-gray-500 mb-4 sm:mb-6 font-mono break-all">{detail.transaction.trace_id}</p>
                <PipelineView events={detail.events} currentState={detail.transaction.state} />
                {detail.proof && (
                  <ProofBundleView proof={detail.proof} />
                )}
                <EventTimeline
                  events={detail.events}
                  nonce={detail.transaction.nonce}
                  analysis={analysisHook}
                />
              </div>
              {/* Analysis panel */}
              {(analysisHook.analysis || analysisHook.loading || analysisHook.error) && (
                <div className="lg:w-1/2 overflow-y-auto border-t lg:border-t-0 border-gray-800">
                  <AnalysisPanel analysis={analysisHook} />
                </div>
              )}
            </div>
          ) : (
            <div className="flex flex-col items-center justify-center h-full text-gray-500 p-6">
              <svg className="w-12 h-12 sm:w-16 sm:h-16 mb-4 opacity-30" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M13 10V3L4 14h7v7l9-11h-7z" />
              </svg>
              <p className="text-sm sm:text-base">Select a transaction to view its lifecycle</p>
            </div>
          )}
        </main>

        {/* Right — real-time event stream */}
        <aside className={`w-full md:w-80 border-l border-gray-800 overflow-y-auto bg-gray-900/50 ${mobileTab !== 'events' ? 'hidden md:block' : ''}`}>
          <div className="p-3 border-b border-gray-800 text-xs font-semibold text-gray-400 uppercase tracking-wider hidden md:block">
            Live Event Stream
          </div>
          {events.length === 0 ? (
            <div className="p-4 text-sm text-gray-500">Waiting for events...</div>
          ) : (
            events.map((ev, i) => <EventCard key={`${ev.nonce}-${ev.step}-${i}`} event={ev} />)
          )}
        </aside>
      </div>
    </div>
  );
}

// ──────────────────────────────────────────────
// Sub-components
// ──────────────────────────────────────────────

function MetricsBar({ metrics }: { metrics: ReturnType<typeof useMetrics> }) {
  return (
    <div className="flex gap-4 text-xs">
      <Stat label="Total" value={metrics.total_transactions} />
      <Stat label="Settled" value={metrics.settled} color="#22c55e" />
      <Stat label="Failed" value={metrics.failed} color="#ef4444" />
      <Stat label="Pending" value={metrics.pending} color="#f59e0b" />
      <Stat label="Retries" value={metrics.total_retries} color="#8b5cf6" />
    </div>
  );
}

function Stat({ label, value, color }: { label: string; value: number; color?: string }) {
  return (
    <div className="text-center">
      <div className="font-bold text-base" style={{ color: color || '#e2e8f0' }}>{value}</div>
      <div className="text-gray-500">{label}</div>
    </div>
  );
}

function TransactionRow({
  tx, selected, onClick,
}: { tx: CrossChainMessage; selected: boolean; onClick: () => void }) {
  const stateColor = tx.state === 'settled' ? '#22c55e'
    : tx.state === 'failed' || tx.state === 'rolled_back' ? '#ef4444'
    : '#f59e0b';

  return (
    <button
      onClick={onClick}
      className={`w-full text-left px-4 py-3 border-b border-gray-800/50 hover:bg-gray-800/50 transition ${
        selected ? 'bg-gray-800 border-l-2 border-l-blue-500' : ''
      }`}
    >
      <div className="flex items-center justify-between mb-1">
        <span className="font-mono text-sm font-bold">#{tx.nonce}</span>
        <span
          className="text-xs px-2 py-0.5 rounded-full font-medium"
          style={{ background: stateColor + '20', color: stateColor }}
        >
          {tx.state}
        </span>
      </div>
      {tx.description ? (
        <div className="text-xs text-gray-300 truncate mb-0.5">{tx.description}</div>
      ) : (
        <div className="text-xs text-gray-500 truncate font-mono">{tx.sender}</div>
      )}
      <div className="text-xs text-gray-400 mt-1">{tx.amount} wei</div>
    </button>
  );
}

function PipelineView({ events, currentState }: { events: LifecycleEvent[]; currentState: string }) {
  const completedSteps = new Set(events.filter(e => e.status === 'success').map(e => e.step));

  return (
    <div className="mb-6 sm:mb-8">
      <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3 sm:mb-4">Lifecycle Pipeline</h3>
      <div className="flex items-center gap-0.5 sm:gap-1 overflow-x-auto pb-2">
        {PIPELINE_STEPS.map((step, i) => {
          const done = completedSteps.has(step);
          const actor = STEP_ACTORS[step];
          const color = ACTOR_COLORS[actor];
          const isLast = i === PIPELINE_STEPS.length - 1;

          return (
            <div key={step} className="flex items-center flex-shrink-0">
              <div className="flex flex-col items-center">
                <div
                  className={`w-7 h-7 sm:w-10 sm:h-10 rounded-full flex items-center justify-center text-[9px] sm:text-xs font-bold border-2 transition-all ${
                    done ? 'step-active' : 'opacity-40'
                  }`}
                  style={{
                    borderColor: color,
                    background: done ? color + '30' : 'transparent',
                    color: done ? color : '#64748b',
                  }}
                >
                  {done ? '✓' : i + 1}
                </div>
                <span className="text-[8px] sm:text-[10px] mt-1 text-gray-400 capitalize">{step}</span>
                <span className="text-[7px] sm:text-[9px] text-gray-600 capitalize">{actor}</span>
              </div>
              {!isLast && (
                <div
                  className="w-4 sm:w-8 h-0.5 mx-0.5 sm:mx-1 mt-[-18px]"
                  style={{ background: done ? color : '#334155' }}
                />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function SimulationControls({
  running, remainingSeconds, startSimulation, stopSimulation, clearData,
}: ReturnType<typeof useSimulation>) {
  const mins = Math.floor(remainingSeconds / 60);
  const secs = remainingSeconds % 60;
  return (
    <div className="flex items-center gap-2 ml-4 pl-4 border-l border-gray-700">
      {running ? (
        <>
          <button
            onClick={stopSimulation}
            className="px-3 py-1 rounded text-xs font-medium bg-red-900/60 hover:bg-red-800/80 text-red-300 transition"
          >
            Stop
          </button>
          <span className="text-xs text-gray-400 font-mono">
            {mins}:{secs.toString().padStart(2, '0')} left
          </span>
        </>
      ) : (
        <button
          onClick={() => startSimulation(60)}
          className="px-3 py-1 rounded text-xs font-medium bg-green-900/60 hover:bg-green-800/80 text-green-300 transition"
        >
          Start (1hr)
        </button>
      )}
      <button
        onClick={() => { if (confirm('Clear all demo data?')) clearData(); }}
        className="px-3 py-1 rounded text-xs font-medium bg-gray-800 hover:bg-gray-700 text-gray-400 transition"
        title="Clear all transactions and events"
      >
        Clear
      </button>
    </div>
  );
}

function ProofBundleView({ proof }: { proof: NonNullable<ReturnType<typeof useTransactionDetail>>['proof'] }) {
  if (!proof) return null;
  return (
    <div className="mb-6 sm:mb-8 bg-gray-900 border border-gray-800 rounded-lg p-3 sm:p-4">
      <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-2 sm:mb-3">
        ECDSA-Signed Proof Bundle
      </h3>
      <p className="text-xs text-green-500/80 mb-2 sm:mb-3">
        Real ECDSA: Validator signature is cryptographically signed and verified via ecrecover.
      </p>
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 text-xs">
        <Field label="Block Header (SHA-256)" value={proof.block_header} />
        <Field label="Event Root (SHA-256)" value={proof.event_root} />
        <Field label="ECDSA Signature (65 bytes)" value={proof.validator_signature} />
        <Field label="Relayer Address (signer)" value={proof.relayer_address} />
        <div className="sm:col-span-2">
          <div className="text-gray-500 mb-1">Merkle Inclusion Proof ({proof.inclusion_proof.length} nodes)</div>
          {proof.inclusion_proof.map((node, i) => (
            <div key={i} className="font-mono text-gray-300 truncate text-[10px]">{node}</div>
          ))}
        </div>
      </div>
    </div>
  );
}

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-gray-500 mb-1">{label}</div>
      <div className="font-mono text-gray-300 truncate text-[10px]">{value}</div>
    </div>
  );
}

function EventTimeline({ events, nonce, analysis }: {
  events: LifecycleEvent[];
  nonce: number;
  analysis: ReturnType<typeof useAnalysis>;
}) {
  return (
    <div>
      <div className="flex items-center gap-4 mb-4">
        <h3 className="text-sm font-semibold text-gray-400 uppercase tracking-wider">Event Timeline</h3>
        <button
          onClick={() => analysis.analyze(nonce)}
          disabled={analysis.loading}
          className="px-3 py-1 rounded text-xs font-semibold bg-gradient-to-r from-purple-600 to-pink-600 hover:from-purple-500 hover:to-pink-500 text-white transition disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {analysis.loading && analysis.analyzedNonce === nonce ? (
            <span className="flex items-center gap-1.5">
              <svg className="w-3 h-3 animate-spin" viewBox="0 0 24 24" fill="none">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
              </svg>
              Analyzing...
            </span>
          ) : 'Analyze'}
        </button>
        {analysis.analysis && (
          <button
            onClick={analysis.clear}
            className="px-2 py-1 rounded text-xs text-gray-400 hover:text-gray-200 bg-gray-800 hover:bg-gray-700 transition"
          >
            Close
          </button>
        )}
      </div>
      <div className="space-y-2">
        {events.map((ev, i) => (
          <div
            key={`${ev.step}-${i}`}
            className="flex items-start gap-3 bg-gray-900/60 border border-gray-800 rounded p-2 sm:p-3"
          >
            <div
              className="w-2 h-2 rounded-full mt-1.5 flex-shrink-0"
              style={{ background: STATUS_COLORS[ev.status] }}
            />
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 mb-1 flex-wrap">
                <span
                  className="text-xs font-bold uppercase"
                  style={{ color: ACTOR_COLORS[ev.actor] }}
                >
                  {ev.actor}
                </span>
                <span className="text-xs text-gray-400 capitalize">{ev.step}</span>
                <span
                  className="text-[10px] px-1.5 py-0.5 rounded font-medium"
                  style={{ background: STATUS_COLORS[ev.status] + '20', color: STATUS_COLORS[ev.status] }}
                >
                  {ev.status}
                </span>
              </div>
              {ev.detail && (
                <div className="text-[10px] text-gray-500 font-mono truncate">{ev.detail}</div>
              )}
              <div className="text-[10px] text-gray-600 mt-1">
                {new Date(ev.timestamp).toLocaleTimeString()}
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function AnalysisPanel({ analysis }: { analysis: ReturnType<typeof useAnalysis> }) {
  return (
    <div className="p-4 sm:p-6">
      <h3 className="text-sm font-semibold text-purple-400 uppercase tracking-wider mb-4 flex items-center gap-2">
        <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z" />
        </svg>
        AI Analysis
      </h3>
      {analysis.loading && (
        <div className="flex items-center gap-3 text-gray-400 text-sm">
          <svg className="w-5 h-5 animate-spin text-purple-400" viewBox="0 0 24 24" fill="none">
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
          </svg>
          Running GPT-4o analysis...
        </div>
      )}
      {analysis.error && (
        <div className="bg-red-900/30 border border-red-800 rounded p-3 text-sm text-red-300">
          {analysis.error}
        </div>
      )}
      {analysis.analysis && (
        <div className="prose prose-invert prose-sm max-w-none
          prose-headings:text-gray-200 prose-h1:text-lg prose-h2:text-base prose-h3:text-sm
          prose-p:text-gray-300 prose-p:text-xs prose-p:leading-relaxed
          prose-li:text-gray-300 prose-li:text-xs
          prose-strong:text-gray-100
          prose-table:text-xs prose-th:text-gray-400 prose-td:text-gray-300
          prose-th:border-gray-700 prose-td:border-gray-800
          prose-th:px-2 prose-th:py-1 prose-td:px-2 prose-td:py-1
          prose-code:text-purple-300 prose-code:text-[10px]
          prose-a:text-blue-400
        ">
          <ReactMarkdown>{analysis.analysis}</ReactMarkdown>
        </div>
      )}
    </div>
  );
}

function EventCard({ event }: { event: LifecycleEvent }) {
  return (
    <div className="px-3 py-2 border-b border-gray-800/50 hover:bg-gray-800/30 transition">
      <div className="flex items-center gap-2 mb-0.5">
        <div
          className="w-1.5 h-1.5 rounded-full"
          style={{ background: STATUS_COLORS[event.status] }}
        />
        <span className="text-[10px] font-bold uppercase" style={{ color: ACTOR_COLORS[event.actor] }}>
          {event.actor}
        </span>
        <span className="text-[10px] text-gray-400 capitalize">{event.step}</span>
        <span className="text-[10px] text-gray-600 ml-auto font-mono">#{event.nonce}</span>
      </div>
      {event.detail && (
        <div className="text-[10px] text-gray-500 font-mono truncate pl-4">{event.detail}</div>
      )}
    </div>
  );
}
