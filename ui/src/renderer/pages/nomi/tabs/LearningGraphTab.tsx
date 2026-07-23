

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Spin, Tag, Empty } from '@arco-design/web-react';
import { ipcBridge } from '@/common';

interface LearningGraphNode {
  id: string;
  label: string;
  node_type: string;
  status: string;
  kind: string;
  strength: number;
  usage_count: number;
  importance: number;
}

interface LearningGraphEdge {
  source: string;
  target: string;
  edge_type: string;
}

interface LearningGraph {
  nodes: LearningGraphNode[];
  edges: LearningGraphEdge[];
}

const NODE_COLORS: Record<string, string> = {
  skill: '#165dff',
  memory: '#00b42a',
  suggestion: '#ff7d00',
};

const EDGE_COLORS: Record<string, string> = {
  reinforces: '#00b42a',
  supersedes: '#f53f3f',
  suggests: '#ff7d00',
  provenance: '#86909c',
};

const LearningGraphTab: React.FC<{ companionId: string }> = ({ companionId }) => {
  const { t } = useTranslation();
  const [graph, setGraph] = useState<LearningGraph | null>(null);
  const [loading, setLoading] = useState(true);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animationRef = useRef<number>(0);

  const fetchGraph = useCallback(async () => {
    setLoading(true);
    try {
      const data = await ipcBridge.companion.learningGraph.invoke({ companion_id: companionId });
      setGraph(data ?? null);
    } catch (err) {
      console.error('Failed to fetch learning graph:', err);
    } finally {
      setLoading(false);
    }
  }, [companionId]);

  useEffect(() => {
    fetchGraph();
  }, [fetchGraph]);

  // Simple force-directed graph rendering on canvas
  useEffect(() => {
    if (!graph || graph.nodes.length === 0) return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const width = canvas.width;
    const height = canvas.height;

    // Initialize node positions
    const positions = new Map<string, { x: number; y: number; vx: number; vy: number }>();
    graph.nodes.forEach((node, i) => {
      const angle = (i / graph.nodes.length) * Math.PI * 2;
      positions.set(node.id, {
        x: width / 2 + Math.cos(angle) * 150,
        y: height / 2 + Math.sin(angle) * 150,
        vx: 0,
        vy: 0,
      });
    });

    const nodeMap = new Map(graph.nodes.map((n) => [n.id, n]));

    const animate = () => {
      ctx.clearRect(0, 0, width, height);

      // Repulsion between nodes
      const nodeIds = Array.from(positions.keys());
      for (let i = 0; i < nodeIds.length; i++) {
        for (let j = i + 1; j < nodeIds.length; j++) {
          const a = positions.get(nodeIds[i])!;
          const b = positions.get(nodeIds[j])!;
          const dx = b.x - a.x;
          const dy = b.y - a.y;
          const dist = Math.sqrt(dx * dx + dy * dy) || 1;
          const force = 2000 / (dist * dist);
          a.vx -= (dx / dist) * force;
          a.vy -= (dy / dist) * force;
          b.vx += (dx / dist) * force;
          b.vy += (dy / dist) * force;
        }
      }

      // Attraction along edges
      graph.edges.forEach((edge) => {
        const a = positions.get(edge.source);
        const b = positions.get(edge.target);
        if (!a || !b) return;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const dist = Math.sqrt(dx * dx + dy * dy) || 1;
        const force = (dist - 120) * 0.01;
        a.vx += (dx / dist) * force;
        a.vy += (dy / dist) * force;
        b.vx -= (dx / dist) * force;
        b.vy -= (dy / dist) * force;
      });

      // Center gravity
      positions.forEach((pos) => {
        pos.vx += (width / 2 - pos.x) * 0.001;
        pos.vy += (height / 2 - pos.y) * 0.001;
      });

      // Update positions
      positions.forEach((pos) => {
        pos.x += pos.vx * 0.5;
        pos.y += pos.vy * 0.5;
        pos.vx *= 0.9;
        pos.vy *= 0.9;
        // Keep in bounds
        pos.x = Math.max(30, Math.min(width - 30, pos.x));
        pos.y = Math.max(20, Math.min(height - 20, pos.y));
      });

      // Draw edges
      graph.edges.forEach((edge) => {
        const a = positions.get(edge.source);
        const b = positions.get(edge.target);
        if (!a || !b) return;
        ctx.strokeStyle = EDGE_COLORS[edge.edge_type] ?? '#999';
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.lineTo(b.x, b.y);
        ctx.stroke();
      });

      // Draw nodes
      graph.nodes.forEach((node) => {
        const pos = positions.get(node.id);
        if (!pos) return;
        const color = NODE_COLORS[node.node_type] ?? '#999';
        const radius = Math.max(4, Math.min(12, node.strength * 12 + 4));

        ctx.fillStyle = color;
        ctx.beginPath();
        ctx.arc(pos.x, pos.y, radius, 0, Math.PI * 2);
        ctx.fill();

        ctx.fillStyle = '#333';
        ctx.font = '10px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText(node.label.slice(0, 20), pos.x, pos.y - radius - 4);
      });

      animationRef.current = requestAnimationFrame(animate);
    };

    animate();
    return () => cancelAnimationFrame(animationRef.current);
  }, [graph]);

  if (loading) {
    return (
      <div style={{ display: 'flex', justifyContent: 'center', padding: '40px' }}>
        <Spin tip={t('Loading...')} />
      </div>
    );
  }

  if (!graph || graph.nodes.length === 0) {
    return <Empty description={t('No learning data yet')} />;
  }

  return (
    <div>
      <div style={{ marginBottom: 12, display: 'flex', gap: 12 }}>
        <Tag color="blue">{t('Skills')}: {graph.nodes.filter((n) => n.node_type === 'skill').length}</Tag>
        <Tag color="green">{t('Memories')}: {graph.nodes.filter((n) => n.node_type === 'memory').length}</Tag>
        <Tag color="orange">{t('Suggestions')}: {graph.nodes.filter((n) => n.node_type === 'suggestion').length}</Tag>
      </div>
      <canvas
        ref={canvasRef}
        width={800}
        height={500}
        style={{ border: '1px solid #e5e6eb', borderRadius: 4, width: '100%' }}
      />
    </div>
  );
};

export default LearningGraphTab;
