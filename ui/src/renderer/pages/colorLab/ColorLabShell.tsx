import { BookOpen, MessageOne, SettingTwo, Time } from '@icon-park/react';
import type { FC } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './colorLab.module.css';
import { tokensToStyle, type ColorLabTokens } from './palettes';

type ColorLabShellProps = {
  tokens: ColorLabTokens;
  caption?: string;
};

const SESSIONS = [
  { id: '1', active: true },
  { id: '2', active: false },
  { id: '3', active: false },
] as const;

const ColorLabShell: FC<ColorLabShellProps> = ({ tokens, caption }) => {
  const { t } = useTranslation();

  return (
    <div>
      {caption ? <p className={styles.compareCaption}>{caption}</p> : null}
      <div className={styles.shell} style={tokensToStyle(tokens)} data-testid='color-lab-shell'>
        <div className={styles.titlebar}>
          <div className={styles.dots} aria-hidden='true'>
            <span className={styles.dot} />
            <span className={styles.dot} />
            <span className={styles.dot} />
          </div>
          <span>{t('colorLab.sessions.1.title')}</span>
        </div>
        <div className={styles.body}>
          <nav className={styles.rail} aria-label={t('colorLab.nav.chat')}>
            <div className={styles.railGroup}>
              <span className={styles.navBtn} aria-current='page' title={t('colorLab.nav.chat')}>
                <MessageOne theme='outline' size={18} fill='currentColor' />
              </span>
              <span className={styles.navBtn} title={t('colorLab.nav.knowledge')}>
                <BookOpen theme='outline' size={18} fill='currentColor' />
              </span>
              <span className={styles.navBtn} title={t('colorLab.nav.scheduled')}>
                <Time theme='outline' size={18} fill='currentColor' />
              </span>
            </div>
            <span className={styles.navBtn} title={t('colorLab.nav.settings')}>
              <SettingTwo theme='outline' size={18} fill='currentColor' />
            </span>
          </nav>
          <aside className={styles.sessions}>
            <div className={styles.sessionHead}>
              <p className={styles.sectionTitle}>{t('colorLab.shell.sessions')}</p>
              <button type='button' className={styles.newChat}>
                {t('colorLab.shell.newChat')}
              </button>
            </div>
            <div className={styles.sessionList}>
              {SESSIONS.map((session) => (
                <div key={session.id} className={styles.session} data-active={session.active ? 'true' : 'false'}>
                  <span className={styles.sessionTitle}>{t(`colorLab.sessions.${session.id}.title`)}</span>
                  <span className={styles.sessionMeta}>{t(`colorLab.sessions.${session.id}.meta`)}</span>
                </div>
              ))}
            </div>
          </aside>
          <section className={styles.chat}>
            <div className={styles.messages}>
              <p className={styles.user}>{t('colorLab.messages.user')}</p>
              <p className={styles.assistant}>{t('colorLab.messages.assistant')}</p>
              <div className={styles.toolChip}>
                <span className={styles.toolDot} aria-hidden='true' />
                {t('colorLab.shell.toolChip')}
              </div>
              <div className={styles.approval}>
                <p className={styles.approvalTitle}>{t('colorLab.shell.approvalTitle')}</p>
                <p className={styles.approvalBody}>{t('colorLab.shell.approvalBody')}</p>
                <div className={styles.approvalActions}>
                  <button type='button' className={styles.approve}>
                    {t('colorLab.shell.approve')}
                  </button>
                  <button type='button' className={styles.skip}>
                    {t('colorLab.shell.skip')}
                  </button>
                </div>
              </div>
            </div>
            <div className={styles.composer}>
              <div className={styles.composerInput}>{t('colorLab.shell.composer')}</div>
              <button type='button' className={styles.send}>
                {t('colorLab.shell.send')}
              </button>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
};

export default ColorLabShell;
