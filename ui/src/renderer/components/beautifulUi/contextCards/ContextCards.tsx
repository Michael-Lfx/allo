import { File, FileCode2, FileSpreadsheet, FileText } from 'lucide-react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import type { ContextCardItem, ContextCardSourceKind } from './contextCardModel';
import styles from './contextCards.module.css';

export type { ContextCardItem, ContextCardSourceKind } from './contextCardModel';
export { sourceKindFromPath } from './contextCardModel';

export type ContextCardsProps = {
  items: ContextCardItem[];
};

const sourceKindLabelKey = (kind: ContextCardSourceKind) => {
  switch (kind) {
    case 'pdf':
      return 'beautifulUiPreview.contextCardSourceKinds.pdf' as const;
    case 'csv':
      return 'beautifulUiPreview.contextCardSourceKinds.csv' as const;
    case 'md':
      return 'beautifulUiPreview.contextCardSourceKinds.md' as const;
    case 'code':
      return 'beautifulUiPreview.contextCardSourceKinds.code' as const;
    case 'other':
      return 'beautifulUiPreview.contextCardSourceKinds.other' as const;
    default: {
      const exhaustive: never = kind;
      return exhaustive;
    }
  }
};

const sourceKindIcon = (kind: ContextCardSourceKind): React.ReactNode => {
  const props = { size: 14, strokeWidth: 1.75, 'aria-hidden': true as const };
  switch (kind) {
    case 'pdf':
      return <FileText {...props} />;
    case 'csv':
      return <FileSpreadsheet {...props} />;
    case 'md':
      return <FileText {...props} />;
    case 'code':
      return <FileCode2 {...props} />;
    case 'other':
      return <File {...props} />;
    default: {
      const exhaustive: never = kind;
      return exhaustive;
    }
  }
};

const ContextCard: React.FC<{ item: ContextCardItem }> = ({ item }) => {
  const { t } = useTranslation();
  const clickable = Boolean(item.onOpen);
  const body = (
    <>
      <div className={styles.header}>
        <p className={styles.title}>{item.title}</p>
        <span className={styles.kind} data-kind={item.sourceKind}>
          <span className={styles.kindIcon} aria-hidden='true'>
            {sourceKindIcon(item.sourceKind)}
          </span>
          {t(sourceKindLabelKey(item.sourceKind))}
        </span>
      </div>
      {item.snippet ? <p className={styles.snippet}>{item.snippet}</p> : null}
      <p className={styles.source}>{item.sourceLabel}</p>
    </>
  );

  return (
    <li className={styles.item} data-testid='beautiful-ui-context-card'>
      {clickable ? (
        <button type='button' className={styles.card} onClick={item.onOpen}>
          {body}
        </button>
      ) : (
        <div className={styles.card}>{body}</div>
      )}
    </li>
  );
};

const ContextCards: React.FC<ContextCardsProps> = ({ items }) => {
  const { t } = useTranslation();

  if (items.length === 0) {
    return <p className={styles.empty}>{t('beautifulUiPreview.emptyChunks')}</p>;
  }

  return (
    <ul className={styles.list}>
      {items.map((item) => (
        <ContextCard key={item.id} item={item} />
      ))}
    </ul>
  );
};

export default ContextCards;
