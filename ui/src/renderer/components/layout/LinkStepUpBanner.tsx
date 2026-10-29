import React, { useEffect, useState } from 'react';
import { Button } from '@arco-design/web-react';
import { link } from '@/common/adapter/ipcBridge';

type StepUpItem = { id: string; method: string; path: string };

export default function LinkStepUpBanner() {
  const [items, setItems] = useState<StepUpItem[]>([]);

  // useEffect必要性：桌面壳轮询本机 Link 二次确认队列；目的：手机发起敏感操作时，不依赖用户正停在「手机版」页。未使用 ahooks：allo 界面工程没有 ahooks。
  useEffect(() => {
    let stopped = false;
    const pull = () => {
      void link.stepUpPending
        .invoke()
        .then((result) => {
          if (!stopped) setItems(result?.items ?? []);
        })
        .catch(() => {
          if (!stopped) setItems([]);
        });
    };
    pull();
    const timer = window.setInterval(pull, 2000);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, []);

  if (items.length === 0) return null;

  return (
    <div className='px-12px py-8px border-b border-solid border-arco-2 bg-[var(--color-bg-1)]'>
      {items.map((item) => (
        <div key={item.id} className='flex items-center gap-8px py-4px'>
          <div className='min-w-0 flex-1 text-13px'>
            手机请求确认
            <span className='ml-8px text-t-tertiary' style={{ fontVariantNumeric: 'tabular-nums' }}>
              {item.method} {item.path}
            </span>
          </div>
          <Button
            type='primary'
            style={{ minHeight: 44 }}
            onClick={() => void link.stepUpDecide.invoke({ id: item.id, allow: true }).then(() => setItems((current) => current.filter((row) => row.id !== item.id)))}
          >
            允许这次
          </Button>
          <Button
            style={{ minHeight: 44 }}
            onClick={() => void link.stepUpDecide.invoke({ id: item.id, allow: false }).then(() => setItems((current) => current.filter((row) => row.id !== item.id)))}
          >
            拒绝
          </Button>
        </div>
      ))}
    </div>
  );
}
