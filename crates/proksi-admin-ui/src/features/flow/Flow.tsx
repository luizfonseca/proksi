import { useEffect, useRef, useState } from 'preact/hooks';
import { ms, percent, rate } from '../../lib/format';
import { type Card, type Flow as FlowData, type FlowEdge, type FlowNode, type Focus, NODE_HEIGHT } from './flowGraph';
import { useChanged } from '../../lib/motion';

/** A title that fits its node (about 6.8px a character) */
function fit(text: string, width: number): string {
  const max = Math.floor((width - 24) / 6.8);
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

/** Busier lines are thicker and move faster */
function edgeStyle(rate: number) {
  const width = rate > 0 ? 2 + Math.min(4, Math.log2(rate + 1)) : 1.5;
  const seconds = rate > 0 ? Math.min(2.4, Math.max(0.35, 3 / Math.log2(rate + 2))) : 0;
  return { width, seconds };
}

/** A line from a node to the next */
function Edge({ edge }: { edge: FlowEdge }) {
  const x1 = edge.from.x + edge.from.width;
  const x2 = edge.to.x;
  const bend = (x2 - x1) / 2;
  const d = `M${x1},${edge.from.y} C${x1 + bend},${edge.from.y} ${x2 - bend},${edge.to.y} ${x2},${edge.to.y}`;
  const { width, seconds } = edgeStyle(edge.rate);
  return (
    <g class={`edge ${edge.tone}`}>
      <path class="track" d={d} stroke-width={width} />
      {edge.rate > 0 && <path class="flow" d={d} stroke-width={width} style={{ animationDuration: `${seconds}s` }} />}
    </g>
  );
}

const same = (a: Focus | null, b: Focus | undefined) =>
  Boolean(a && b && a.location === b.location && a.upstreams?.[0] === b.upstreams?.[0]);

function Node({
  node,
  selected,
  onHover,
  onSelect,
}: {
  node: FlowNode;
  selected: boolean;
  onHover: (node: FlowNode | null) => void;
  onSelect?: (node: FlowNode) => void;
}) {
  const top = node.y - NODE_HEIGHT / 2;
  const select = onSelect && node.filter ? () => onSelect(node) : undefined;
  // Its border pulses once when its state changes (flowing to failing, or back)
  const changed = useChanged(node.tone);
  return (
    <g
      class={`node ${node.tone} ${select ? 'selectable' : ''} ${selected ? 'selected' : ''} ${changed ? 'changed' : ''}`}
      onMouseEnter={() => onHover(node)}
      onMouseLeave={() => onHover(null)}
      onClick={select}
      onKeyDown={(event) => select && (event.key === 'Enter' || event.key === ' ') && (event.preventDefault(), select())}
      role={select ? 'button' : undefined}
      tabIndex={select ? 0 : undefined}
      aria-label={select ? `Show the requests to ${node.title}` : undefined}
      aria-pressed={select ? selected : undefined}
    >
      <rect x={node.x} y={top} width={node.width} height={NODE_HEIGHT} rx="9" />
      <circle cx={node.x + 14} cy={top + 16} r="4" />
      <text class="title" x={node.x + 26} y={top + 20}>
        {fit(node.title, node.width - 14)}
      </text>
      <text class="detail" x={node.x + 12} y={top + 36}>
        {fit(node.detail, node.width)}
      </text>
    </g>
  );
}

const CLASSES = [
  ['2xx', 'bg-success'],
  ['3xx', 'bg-info'],
  ['4xx', 'bg-warning'],
  ['5xx', 'bg-error'],
] as const;

/** Responses by status class, as one bar, and their counts */
function Statuses({ responses }: { responses: Record<string, number> }) {
  const all = CLASSES.reduce((sum, [status]) => sum + (responses[status] ?? 0), 0);
  if (all === 0) return null;
  return (
    <div class="grid gap-1.5">
      <div class="flex h-1.5 overflow-hidden rounded-full bg-base-300">
        {CLASSES.map(([status, color]) => (responses[status] ? <span class={color} style={{ width: `${(responses[status] / all) * 100}%` }} /> : null))}
      </div>
      <div class="flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-base-content/60">
        {CLASSES.map(([status, color]) =>
          responses[status] ? (
            <span class="inline-flex items-center gap-1">
              <i class={`inline-block size-1.5 rounded-full ${color}`} />
              {status} {percent((responses[status] / all) * 100)}
            </span>
          ) : null,
        )}
      </div>
    </div>
  );
}

/** What a hovered node shows: what goes through it, how it answers, how long it takes */
function HoverCard({ node, card, flow, pad }: { node: FlowNode; card: Card; flow: FlowData; pad: number }) {
  // Below the node, or above it in the bottom half, so it stays in the graph
  const below = node.y < flow.height / 2;
  const left = Math.min(node.x, flow.width - 260);
  const px = (value: number) => `${Math.round(value)}px`;
  const style = below
    ? { left: px(left), top: px(node.y + pad + NODE_HEIGHT / 2 + 6) }
    : { left: px(left), bottom: px(flow.height + pad - (node.y - NODE_HEIGHT / 2) + 6) };
  const quantiles = card.duration && (['p50', 'p90', 'p99'] as const).filter((quantile) => card.duration?.[quantile] !== null);
  return (
    <div class="pointer-events-none absolute z-10 grid w-64 gap-2.5 rounded-box border border-base-300 bg-base-100 p-3 text-sm shadow-xl" style={style}>
      <div class="font-semibold break-all">{node.title}</div>
      <div class="flex flex-wrap gap-x-3 text-xs text-base-content/60">
        <span>
          <b class="text-base-content">{rate(card.requests)}</b> answered
        </span>
        {card.failed ? (
          <span>
            <b class="text-error">{rate(card.failed)}</b> failed
          </span>
        ) : null}
        {node.share !== undefined && node.share > 0 && <span>{percent(node.share * 100)} of its location</span>}
        {card.health && <span class={card.health === 'healthy' ? 'text-success' : 'text-error'}>{card.health}</span>}
      </div>
      {card.responses && <Statuses responses={card.responses} />}
      {quantiles && quantiles.length > 0 && (
        <div class="grid grid-cols-3 gap-2">
          {quantiles.map((quantile) => (
            <div>
              <div class="text-xs text-base-content/45">{quantile}</div>
              <div class="font-mono text-sm whitespace-nowrap">{ms(card.duration?.[quantile] ?? 0)}</div>
            </div>
          ))}
        </div>
      )}
      {node.filter && <div class="text-xs text-base-content/45">Click to see its requests</div>}
    </div>
  );
}

/**
 * A host's traffic, from clients to upstreams, over the last seconds: `draw` lays it out for the
 * width it has, drawn at its actual size. Locations and upstreams can be clicked (`onSelect`), and
 * show more when hovered.
 */
export function Flow({
  draw,
  selected = null,
  onSelect,
}: {
  draw: (width: number) => FlowData;
  selected?: Focus | null;
  onSelect?: (node: FlowNode) => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [hovered, setHovered] = useState<FlowNode | null>(null);
  useEffect(() => {
    const element = box.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => setWidth(Math.floor(entry?.contentRect.width ?? 0)));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const flow = width > 0 ? draw(width) : null;
  const pad = NODE_HEIGHT / 2 + 4;
  // The hovered node as it is now (the graph is drawn again each second)
  const shown = hovered && flow?.nodes.find((node) => node.id === hovered.id);
  return (
    <div ref={box} class="relative overflow-x-auto">
      {flow && (
        <svg
          class={`flow block font-sans transition-opacity ${flow.idle ? 'opacity-50' : ''}`}
          width={flow.width}
          height={flow.height + pad * 2}
          viewBox={`0 ${-pad} ${flow.width} ${flow.height + pad * 2}`}
          role="img"
          aria-label="Traffic from clients to upstreams"
        >
          {flow.edges.map((edge) => (
            <Edge key={`${edge.from.id}>${edge.to.id}`} edge={edge} />
          ))}
          {flow.nodes.map((node) => (
            <Node key={node.id} node={node} selected={same(selected, node.filter)} onHover={setHovered} onSelect={onSelect} />
          ))}
        </svg>
      )}
      {flow?.idle && (
        <div class="pointer-events-none absolute inset-x-0 bottom-2 flex justify-center">
          <span class="rounded-full border border-base-300 bg-base-100 px-3 py-1 text-sm text-base-content/60">No requests right now</span>
        </div>
      )}
      {flow && shown && !flow.idle && <HoverCard node={shown} card={shown.card} flow={flow} pad={pad} />}
    </div>
  );
}
