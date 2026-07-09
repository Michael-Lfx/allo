/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Spin, Card, Grid, Statistic, Tag, Table, Empty, Progress } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type { ILocalAnalytics } from '@/common/adapter/ipcBridge';

const { Row, Col } = Grid;

const AnalyticsTab: React.FC = () => {
  const { t } = useTranslation();
  const [analytics, setAnalytics] = useState<ILocalAnalytics | null>(null);
  const [loading, setLoading] = useState(true);

  const fetchAnalytics = useCallback(async () => {
    setLoading(true);
    try {
      const data = await ipcBridge.companion.localAnalytics.invoke();
      setAnalytics(data ?? null);
    } catch (err) {
      console.error('Failed to fetch local analytics:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchAnalytics();
  }, [fetchAnalytics]);

  if (loading) {
    return (
      <div style={{ display: 'flex', justifyContent: 'center', padding: '40px' }}>
        <Spin tip={t('Loading...')} />
      </div>
    );
  }

  if (!analytics) {
    return <Empty description={t('No analytics data yet')} />;
  }

  const skillStatusTotal = analytics.skills.by_status.active + analytics.skills.by_status.draft + analytics.skills.by_status.archived || 1;

  return (
    <div style={{ padding: '16px' }}>
      {/* Conversation Stats */}
      <Card title={t('Conversations')} size="small" style={{ marginBottom: 16 }}>
        <Row gutter={16}>
          <Col span={6}>
            <Statistic title={t('Total Conversations')} value={analytics.conversations.total_conversations} />
          </Col>
          <Col span={6}>
            <Statistic title={t('Active (7d)')} value={analytics.conversations.active_conversations_7d} />
          </Col>
          <Col span={6}>
            <Statistic title={t('Total Messages')} value={analytics.conversations.total_messages} />
          </Col>
          <Col span={6}>
            <Statistic title={t('Messages (7d)')} value={analytics.conversations.messages_7d} />
          </Col>
        </Row>
      </Card>

      {/* Skill Stats */}
      <Card title={t('Skills')} size="small" style={{ marginBottom: 16 }}>
        <Row gutter={16}>
          <Col span={6}>
            <Statistic title={t('Active')} value={analytics.skills.by_status.active} />
          </Col>
          <Col span={6}>
            <Statistic title={t('Draft')} value={analytics.skills.by_status.draft} />
          </Col>
          <Col span={6}>
            <Statistic title={t('Avg Strength')} value={(analytics.skills.avg_strength * 100).toFixed(1)} suffix="%" />
          </Col>
          <Col span={6}>
            <Statistic title={t('Avg Confidence')} value={(analytics.skills.avg_confidence * 100).toFixed(1)} suffix="%" />
          </Col>
        </Row>

        {/* Skill status distribution */}
        <div style={{ marginTop: 16 }}>
          <div style={{ marginBottom: 8, fontSize: 13, color: '#86909c' }}>{t('Status Distribution')}</div>
          <div style={{ display: 'flex', gap: 16, flexWrap: 'wrap' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
              <span style={{ width: 10, height: 10, borderRadius: '50%', background: '#00b42a', display: 'inline-block' }} />
              <span>{t('Active')}: {analytics.skills.by_status.active}</span>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
              <span style={{ width: 10, height: 10, borderRadius: '50%', background: '#ff7d00', display: 'inline-block' }} />
              <span>{t('Draft')}: {analytics.skills.by_status.draft}</span>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
              <span style={{ width: 10, height: 10, borderRadius: '50%', background: '#86909c', display: 'inline-block' }} />
              <span>{t('Archived')}: {analytics.skills.by_status.archived}</span>
            </div>
          </div>
          <Progress
            percent={Math.round((analytics.skills.by_status.active / skillStatusTotal) * 100)}
            color="#00b42a"
          />
        </div>

        {/* Skill source breakdown */}
        <div style={{ marginTop: 12, display: 'flex', gap: 8 }}>
          <Tag color="blue">{t('Mined')}: {analytics.skills.by_source.mined}</Tag>
          <Tag color="cyan">{t('Manual')}: {analytics.skills.by_source.manual}</Tag>
          <Tag color="purple">{t('Imported')}: {analytics.skills.by_source.imported}</Tag>
        </div>

        {/* Top skills by usage */}
        {analytics.skills.top_by_usage.length > 0 && (
          <div style={{ marginTop: 16 }}>
            <div style={{ marginBottom: 8, fontSize: 13, color: '#86909c' }}>{t('Top Skills by Usage')}</div>
            <Table
              size="small"
              pagination={false}
              data={analytics.skills.top_by_usage.map((s: { name: string; usage_count: number; strength: number }, i: number) => ({ ...s, key: i }))}
              columns={[
                { title: '#', dataIndex: 'key', width: 40, render: (v: number) => v + 1 },
                { title: t('Name'), dataIndex: 'name' },
                { title: t('Usage'), dataIndex: 'usage_count', width: 80 },
                {
                  title: t('Strength'),
                  dataIndex: 'strength',
                  width: 100,
                  render: (v: number) => `${(v * 100).toFixed(0)}%`,
                },
              ]}
            />
          </div>
        )}
      </Card>

      {/* Memory Stats */}
      <Card title={t('Memories')} size="small" style={{ marginBottom: 16 }}>
        <Row gutter={16}>
          <Col span={6}>
            <Statistic title={t('Active Memories')} value={analytics.memories.total_active} />
          </Col>
          <Col span={6}>
            <Statistic title={t('Pinned')} value={analytics.memories.pinned} />
          </Col>
          <Col span={6}>
            <Statistic title={t('Avg Importance')} value={(analytics.memories.avg_importance * 100).toFixed(1)} suffix="%" />
          </Col>
          <Col span={6}>
            <Statistic title={t('Avg Strength')} value={(analytics.memories.avg_strength * 100).toFixed(1)} suffix="%" />
          </Col>
        </Row>

        {/* Memory kind breakdown */}
        <div style={{ marginTop: 12, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <Tag color="blue">{t('Profile')}: {analytics.memories.by_kind.profile}</Tag>
          <Tag color="green">{t('Preference')}: {analytics.memories.by_kind.preference}</Tag>
          <Tag color="orange">{t('Knowledge')}: {analytics.memories.by_kind.knowledge}</Tag>
          <Tag color="purple">{t('Episode')}: {analytics.memories.by_kind.episode}</Tag>
          <Tag color="red">{t('Task')}: {analytics.memories.by_kind.task}</Tag>
          <Tag color="pink">{t('Affective')}: {analytics.memories.by_kind.affective}</Tag>
        </div>
      </Card>

      {/* Learning Stats */}
      <Card title={t('Learning')} size="small" style={{ marginBottom: 16 }}>
        <Row gutter={16}>
          <Col span={6}>
            <Statistic title={t('Total Runs')} value={analytics.learning.total_runs} />
          </Col>
          <Col span={6}>
            <Statistic title={t('Runs (7d)')} value={analytics.learning.runs_7d} />
          </Col>
          <Col span={6}>
            <Statistic title={t('Memories Added')} value={analytics.learning.total_memories_added} />
          </Col>
          <Col span={6}>
            <Statistic title={t('Suggestions Added')} value={analytics.learning.total_suggestions_added} />
          </Col>
        </Row>
      </Card>

      <div style={{ textAlign: 'right', fontSize: 12, color: '#86909c' }}>
        {t('Generated at')}: {new Date(analytics.generated_at).toLocaleString()}
      </div>
    </div>
  );
};

export default AnalyticsTab;
