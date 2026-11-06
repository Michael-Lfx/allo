import { useContext, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import FlipCard from '@renderer/components/motion/FlipCard';
import { formatSiderAccountLabel } from '@renderer/components/layout/Sider/accountLabel';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { CreditsContext } from '@renderer/hooks/context/CreditsContext';
import { formatMembershipDate, membershipDisplayName, shortPlanBadge } from './membershipCardModel';
import layoutStyles from '../index.module.css';
import styles from './GuidHomeCard.module.css';

const PLUS_FIELD = (() => {
  const parts: string[] = [];
  const cols = 19;
  const rows = 8;
  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < cols; col += 1) {
      const x = 14 + col * 23.2;
      const y = 12 + row * 23.2;
      const arm = 3.15;
      parts.push(`M${x} ${y - arm}V${y + arm}M${x - arm} ${y}H${x + arm}`);
    }
  }
  return parts.join('');
})();

const hasCjk = (value: string) => /[\u3400-\u9fff]/.test(value);

function PlusField() {
  return (
    <svg className={styles.stars} viewBox='0 0 470 200' preserveAspectRatio='xMidYMid slice' aria-hidden='true'>
      <path d={PLUS_FIELD} fill='none' stroke='currentColor' strokeWidth='0.95' strokeLinecap='round' />
    </svg>
  );
}

/** Membership seal for the compact card: two concentric rings, an F, one offset node. */
function MemberSeal() {
  return (
    <svg className={styles.seal} viewBox='0 0 96 96' aria-hidden='true' focusable='false'>
      <circle className={styles.sealRing} cx='48' cy='48' r='43' />
      <circle className={styles.sealRingInner} cx='48' cy='48' r='32' />
      <path className={styles.sealLetter} d='M42 33h15M42 33v30M42 48h11' />
      <circle className={styles.sealNode} cx='61' cy='40' r='3.4' />
    </svg>
  );
}

/** Homepage membership card. Compact cross-two-rows flip card. */
const GuidHomeBrand: React.FC = () => {
  const { t, i18n } = useTranslation();
  const { whoami, status } = useCloudAuth();
  const credits = useContext(CreditsContext);
  const dateLabel = useMemo(() => formatMembershipDate(new Date()), []);
  const slotRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 416, height: 220 });

  useEffect(() => {
    const el = slotRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const measure = () => {
      const rect = el.getBoundingClientRect();
      const width = Math.max(160, Math.round(rect.width));
      const height = Math.max(44, Math.round(rect.height));
      setSize((current) => (current.width === width && current.height === height ? current : { width, height }));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const checking = status === 'checking';
  const signedIn = status === 'authenticated' && !!whoami?.authenticated;
  const accountLabel = signedIn
    ? formatSiderAccountLabel({
        nickname: whoami?.nickname,
        username: whoami?.username,
        email: whoami?.email,
      })
    : '';
  const name = signedIn
    ? membershipDisplayName(accountLabel) || t('guid.home.cardGuest')
    : checking
      ? ''
      : t('guid.home.cardGuest');
  const badge = signedIn ? shortPlanBadge(whoami?.plan, whoami?.planCode) : '';
  const email = signedIn ? (whoami?.email?.trim() ?? '') : '';
  const planLabel = signedIn ? (whoami?.plan?.trim() ?? '') : '';
  const creditsLabel =
    signedIn && credits?.authenticated && credits.balanceConfirmed
      ? t('guid.home.cardCredits', { amount: credits.balance.toLocaleString(i18n.language) })
      : '';
  const detail = [planLabel, creditsLabel].filter(Boolean).join(' · ');
  const showEmail = Boolean(email) && email !== name;
  const wordmark = t('guid.home.cardWordmark');

  return (
    <div className={`${layoutStyles.homeBrand} ${styles.slot}`} ref={slotRef} data-testid='guid-home-brand'>
      <div className={styles.cardFill}>
        <FlipCard
        axis='y'
        flipOnClick
        draggable={false}
        tilt={false}
        glare={false}
        hoverScale={1}
        perspective={1200}
        stiffness={160}
        damping={22}
        width={size.width}
        height={size.height}
        radius={12}
        background='transparent'
        color='#17171a'
        shadow
        shadowColor='#7d92b8'
        shadowOpacity={0.18}
        ariaLabel={t('guid.home.cardFlip')}
        front={
          <div className={styles.front}>
            <PlusField />
            <p className={styles.wordmark}>{wordmark}</p>
            <div className={styles.sealRow}>
              <MemberSeal />
            </div>
            <div className={styles.bottomRow}>
              <div className={styles.identity}>
                {checking ? (
                  <span className={styles.nameHold} />
                ) : (
                  <span className={styles.name}>{name}</span>
                )}
                {badge ? <span className={styles.badge}>{badge}</span> : null}
              </div>
              <span className={styles.date}>{dateLabel}</span>
            </div>
          </div>
        }
        back={
          <div className={styles.back}>
            <p className={styles.watermark} aria-hidden='true'>
              {wordmark}
            </p>
            <p className={styles.quote}>
              <span className={styles.quoteCool}>{t('guid.home.cardQuoteCool')}</span>
              <span className={styles.quoteWarm}>{t('guid.home.cardQuoteWarm')}</span>
            </p>
            {checking ? null : signedIn ? (
              showEmail || detail ? (
                <p className={styles.meta}>
                  {showEmail ? <span className={styles.metaLine}>{email}</span> : null}
                  {detail ? <span className={styles.metaLine}>{detail}</span> : null}
                </p>
              ) : null
            ) : (
              <p className={styles.meta}>
                <span className={styles.metaLine}>{t('guid.home.cardSignInHint')}</span>
              </p>
            )}
            <p className={styles.backWord}>{wordmark}</p>
          </div>
        }
      />
      </div>
    </div>
  );
};

export default GuidHomeBrand;
