import { type Flow as FlowData, type FlowEdge, type FlowNode, NODE_HEIGHT } from './flowGraph';

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

function Node({ node, href }: { node: FlowNode; href?: string }) {
  const top = node.y - NODE_HEIGHT / 2;
  const box = (
    <g class={`node ${node.tone}`}>
      <rect x={node.x} y={top} width={node.width} height={NODE_HEIGHT} rx="9" />
      <circle cx={node.x + 14} cy={top + 16} r="4" />
      <text class="title" x={node.x + 26} y={top + 20}>
        {fit(node.title, node.width - 14)}
      </text>
      <text class="detail" x={node.x + 12} y={top + 36}>
        {fit(node.detail, node.width)}
      </text>
      <title>{`${node.title}\n${node.detail}`}</title>
    </g>
  );
  return href ? <a href={href}>{box}</a> : box;
}

/** A host's traffic, from clients to upstreams, over the last seconds */
export function Flow({ flow }: { flow: FlowData }) {
  const pad = NODE_HEIGHT / 2 + 4;
  return (
    <div class="flow-scroll">
      <svg class="flow" viewBox={`0 ${-pad} ${flow.width} ${flow.height + pad * 2}`} role="img" aria-label="Traffic from clients to upstreams">
        {flow.edges.map((edge) => (
          <Edge key={`${edge.from.id}>${edge.to.id}`} edge={edge} />
        ))}
        {flow.nodes.map((node) => (
          <Node key={node.id} node={node} href={node.id.startsWith('location ') ? `#${encodeURIComponent(node.id)}` : undefined} />
        ))}
      </svg>
    </div>
  );
}
