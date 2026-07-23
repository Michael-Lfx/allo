

import React, { useEffect, useRef, useState } from 'react';

type Node = { x: number; y: number; r: number; pulse: number };

function buildNodes(width: number, height: number): Node[] {
  const nodes: Node[] = [];
  const cols = 5;
  const rows = 6;
  const padX = width * 0.14;
  const padY = height * 0.16;
  const spanX = width - padX * 2;
  const spanY = height - padY * 2;

  for (let row = 0; row < rows; row++) {
    for (let col = 0; col < cols; col++) {
      const jitterX = ((row * 17 + col * 31) % 11) - 5;
      const jitterY = ((row * 23 + col * 13) % 9) - 4;
      nodes.push({
        x: padX + (spanX * col) / (cols - 1) + jitterX,
        y: padY + (spanY * row) / (rows - 1) + jitterY,
        r: 1.6 + ((col + row) % 3) * 0.4,
        pulse: (col * 0.37 + row * 0.53) % 1,
      });
    }
  }
  return nodes;
}

function neighbors(nodes: Node[], index: number, maxDist: number): number[] {
  const out: number[] = [];
  const a = nodes[index];
  for (let i = 0; i < nodes.length; i++) {
    if (i === index) continue;
    const b = nodes[i];
    const dx = a.x - b.x;
    const dy = a.y - b.y;
    if (dx * dx + dy * dy < maxDist * maxDist) out.push(i);
  }
  return out;
}

const LanMesh: React.FC = () => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [dimensions, setDimensions] = useState({ width: 0, height: 0 });
  const reduceMotion =
    typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas?.parentElement) return;

    const resizeObserver = new ResizeObserver((entries) => {
      const { width, height } = entries[0].contentRect;
      setDimensions({ width, height });
      canvas.width = width;
      canvas.height = height;
    });

    resizeObserver.observe(canvas.parentElement);
    return () => resizeObserver.disconnect();
  }, []);

  useEffect(() => {
    if (!dimensions.width || !dimensions.height) return;

    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const nodes = buildNodes(dimensions.width, dimensions.height);
    const maxDist = Math.min(dimensions.width, dimensions.height) * 0.28;
    const links = nodes.map((_, i) => neighbors(nodes, i, maxDist).filter((j) => j > i));
    let frame = 0;
    let start = performance.now();

    const draw = (now: number) => {
      const t = (now - start) / 1000;
      ctx.clearRect(0, 0, dimensions.width, dimensions.height);

      // Soft radial wash
      const wash = ctx.createRadialGradient(
        dimensions.width * 0.45,
        dimensions.height * 0.4,
        20,
        dimensions.width * 0.5,
        dimensions.height * 0.5,
        dimensions.width * 0.7
      );
      wash.addColorStop(0, 'rgba(56, 189, 248, 0.07)');
      wash.addColorStop(1, 'rgba(15, 23, 42, 0)');
      ctx.fillStyle = wash;
      ctx.fillRect(0, 0, dimensions.width, dimensions.height);

      // Links
      ctx.lineWidth = 1;
      for (let i = 0; i < nodes.length; i++) {
        for (const j of links[i]) {
          const a = nodes[i];
          const b = nodes[j];
          const shimmer = reduceMotion ? 0.22 : 0.12 + 0.16 * (0.5 + 0.5 * Math.sin(t * 0.9 + a.pulse * 6));
          ctx.strokeStyle = `rgba(148, 163, 184, ${shimmer})`;
          ctx.beginPath();
          ctx.moveTo(a.x, a.y);
          ctx.lineTo(b.x, b.y);
          ctx.stroke();
        }
      }

      // Nodes
      for (const node of nodes) {
        const glow = reduceMotion ? 0 : 0.35 + 0.35 * Math.sin(t * 1.4 + node.pulse * 8);
        if (glow > 0.5) {
          ctx.beginPath();
          ctx.fillStyle = `rgba(56, 189, 248, ${0.08 + glow * 0.12})`;
          ctx.arc(node.x, node.y, node.r * 4.5, 0, Math.PI * 2);
          ctx.fill();
        }
        ctx.beginPath();
        ctx.fillStyle = 'rgba(226, 232, 240, 0.55)';
        ctx.arc(node.x, node.y, node.r, 0, Math.PI * 2);
        ctx.fill();
      }

      // Packet hops along a few edges
      if (!reduceMotion) {
        const hopPairs: [number, number][] = [
          [0, 1],
          [7, 12],
          [14, 19],
          [22, 27],
        ];
        for (let h = 0; h < hopPairs.length; h++) {
          const [si, ei] = hopPairs[h];
          if (!nodes[si] || !nodes[ei]) continue;
          const progress = (t * 0.35 + h * 0.22) % 1;
          const x = nodes[si].x + (nodes[ei].x - nodes[si].x) * progress;
          const y = nodes[si].y + (nodes[ei].y - nodes[si].y) * progress;
          ctx.beginPath();
          ctx.fillStyle = 'rgba(125, 211, 252, 0.85)';
          ctx.arc(x, y, 2.2, 0, Math.PI * 2);
          ctx.fill();
        }
      }

      if (!reduceMotion) {
        frame = requestAnimationFrame(draw);
      }
    };

    if (reduceMotion) {
      draw(performance.now());
    } else {
      frame = requestAnimationFrame(draw);
    }

    return () => {
      if (frame) cancelAnimationFrame(frame);
    };
  }, [dimensions, reduceMotion]);

  return (
    <div className='login-page__mesh'>
      <canvas ref={canvasRef} className='login-page__mesh-canvas' aria-hidden='true' />
    </div>
  );
};

export default LanMesh;
