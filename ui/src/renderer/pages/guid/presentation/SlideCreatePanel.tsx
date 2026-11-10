/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Alarm,
  Analysis,
  ChartHistogram,
  Close,
  Code,
  DocDetail,
  Experiment,
  Funds,
  Left,
  Magic,
  Peoples,
  Ppt,
  Refresh,
  Right,
  Rocket,
  Upload,
} from '@icon-park/react';
import { fileBaseName, isSameSlideTemplate, type SlideTemplate } from './slideOptions';
import { suggestionPage, type SlideSuggestion, type SlideSuggestionIcon } from './slideGallery';
import { SLIDE_THEME_IDS } from './slideThemes';
import SlideThemeThumbnail from './SlideThemeThumbnail';
import styles from './SlideCreatePanel.module.css';

const SUGGESTION_ICONS: Record<SlideSuggestionIcon, React.FC<{ theme: 'outline'; size: number; fill: string }>> = {
  report: Ppt,
  chart: ChartHistogram,
  analysis: Analysis,
  launch: Rocket,
  funds: Funds,
  code: Code,
  people: Peoples,
  alarm: Alarm,
  experiment: Experiment,
};

type SlideCreatePanelProps = {
  template: SlideTemplate;
  onSelectTemplate: (template: SlideTemplate) => void;
  onSelectSuggestion: (suggestion: SlideSuggestion) => void;
  onImportTemplate: () => void;
  onClose: () => void;
  motion?: 'open' | 'closing';
  onMotionEnd?: () => void;
  disabled?: boolean;
};

/**
 * "Create presentation" panel above the home composer (slides mode): starter
 * topics and a rail of visual themes (ppt-master visual style + palette +
 * typography), plus importing a reference file as a template.
 */
const SlideCreatePanel: React.FC<SlideCreatePanelProps> = ({
  template,
  onSelectTemplate,
  onSelectSuggestion,
  onImportTemplate,
  onClose,
  motion = 'open',
  onMotionEnd,
  disabled,
}) => {
  const { t } = useTranslation();
  const [page, setPage] = useState(0);
  const railRef = useRef<HTMLDivElement>(null);
  const [edges, setEdges] = useState({ start: true, end: false });

  const updateEdges = useCallback(() => {
    const rail = railRef.current;
    if (!rail) return;
    const start = rail.scrollLeft <= 1;
    const end = rail.scrollLeft + rail.clientWidth >= rail.scrollWidth - 1;
    setEdges((current) => (current.start === start && current.end === end ? current : { start, end }));
  }, []);

  useEffect(() => {
    const rail = railRef.current;
    if (!rail) return;
    updateEdges();
    const observer = new ResizeObserver(updateEdges);
    observer.observe(rail);
    return () => observer.disconnect();
  }, [updateEdges]);

  // An imported template is inserted at the front of the rail; bring it into view.
  const importedPath = template.kind === 'import' ? template.path : null;
  useEffect(() => {
    if (importedPath) railRef.current?.scrollTo({ left: 0, behavior: 'smooth' });
  }, [importedPath]);

  const scrollRail = (direction: -1 | 1) => {
    const rail = railRef.current;
    if (!rail) return;
    rail.scrollBy({ left: direction * rail.clientWidth * 0.9, behavior: 'smooth' });
  };

  const renderTemplate = (choice: SlideTemplate, key: string, name: string, title: string, thumb: React.ReactNode) => {
    const selected = isSameSlideTemplate(choice, template);
    return (
      <button
        key={key}
        type='button'
        role='radio'
        aria-checked={selected}
        className={styles.template}
        data-selected={selected || undefined}
        data-testid={`guid-slide-template-${key}`}
        title={title}
        disabled={disabled}
        onClick={() => onSelectTemplate(choice)}
      >
        <span className={styles.thumb}>{thumb}</span>
        <span className={styles.templateName}>{name}</span>
      </button>
    );
  };

  return (
    <section
      className={styles.panel}
      data-testid='guid-slide-create-panel'
      data-motion={motion}
      aria-label={t('presentation.create.title')}
      aria-hidden={motion === 'closing' ? true : undefined}
      onAnimationEnd={(event) => {
        if (event.target === event.currentTarget && motion === 'closing') onMotionEnd?.();
      }}
    >
      <header className={styles.header}>
        <h2 className={styles.title}>{t('presentation.create.title')}</h2>
        <span className={styles.headerActions}>
          <button
            type='button'
            className={styles.textButton}
            data-testid='guid-slide-template-import'
            title={t('presentation.create.importHint')}
            disabled={disabled}
            onClick={onImportTemplate}
          >
            <Upload theme='outline' size={15} fill='currentColor' />
            <span>{t('presentation.create.import')}</span>
          </button>
          <button
            type='button'
            className={styles.iconButton}
            aria-label={t('presentation.create.previous')}
            disabled={edges.start}
            onClick={() => scrollRail(-1)}
          >
            <Left theme='outline' size={16} fill='currentColor' />
          </button>
          <button
            type='button'
            className={styles.iconButton}
            aria-label={t('presentation.create.next')}
            disabled={edges.end}
            onClick={() => scrollRail(1)}
          >
            <Right theme='outline' size={16} fill='currentColor' />
          </button>
          <button
            type='button'
            className={styles.iconButton}
            aria-label={t('presentation.create.refresh')}
            title={t('presentation.create.refresh')}
            data-testid='guid-slide-suggestions-refresh'
            onClick={() => setPage((current) => current + 1)}
          >
            <Refresh theme='outline' size={16} fill='currentColor' />
          </button>
          <button
            type='button'
            className={styles.iconButton}
            aria-label={t('presentation.create.close')}
            title={t('presentation.create.close')}
            data-testid='guid-slide-create-close'
            onClick={onClose}
          >
            <Close theme='outline' size={16} fill='currentColor' />
          </button>
        </span>
      </header>

      <div
        ref={railRef}
        className={styles.rail}
        role='radiogroup'
        aria-label={t('presentation.create.templateTitle')}
        onScroll={updateEdges}
      >
        {template.kind === 'import' &&
          renderTemplate(
            template,
            'import',
            fileBaseName(template.path),
            t('presentation.create.importedHint'),
            <span className={styles.importThumb}>
              <DocDetail theme='outline' size={28} fill='currentColor' />
            </span>
          )}
        {renderTemplate(
          { kind: 'free' },
          'free',
          t('presentation.create.free.name'),
          t('presentation.create.free.description'),
          <span className={styles.freeThumb}>
            <Magic theme='outline' size={28} fill='currentColor' />
          </span>
        )}
        {SLIDE_THEME_IDS.map((id) =>
          renderTemplate(
            { kind: 'theme', id },
            id,
            t(`presentation.create.themes.${id}.name`),
            t(`presentation.create.themes.${id}.description`),
            <SlideThemeThumbnail id={id} className={styles.thumbImage} />
          )
        )}
      </div>

      {/* Topic suggestions stay on one line below the templates so the real
          template covers are the first thing visible in every height. */}
      <div className={styles.suggestions}>
        {suggestionPage(page).map((suggestion) => {
          const Icon = SUGGESTION_ICONS[suggestion.icon];
          return (
            <button
              key={suggestion.id}
              type='button'
              className={styles.suggestion}
              data-testid={`guid-slide-suggestion-${suggestion.id}`}
              disabled={disabled}
              onClick={() => onSelectSuggestion(suggestion)}
              title={t(`presentation.create.suggestions.${suggestion.id}.title`)}
            >
              <span className={styles.suggestionIcon}>
                <Icon theme='outline' size={14} fill='currentColor' />
              </span>
              <span className={styles.suggestionText}>{t(`presentation.create.suggestions.${suggestion.id}.title`)}</span>
            </button>
          );
        })}
      </div>
    </section>
  );
};

export default SlideCreatePanel;
