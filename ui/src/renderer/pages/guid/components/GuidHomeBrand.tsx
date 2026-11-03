import { useContext, useId, useMemo } from 'react';
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
  const cols = 24;
  const rows = 10;
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
    <svg className={styles.stars} viewBox='0 0 560 232' preserveAspectRatio='xMidYMid slice' aria-hidden='true'>
      <path d={PLUS_FIELD} fill='none' stroke='currentColor' strokeWidth='0.95' strokeLinecap='round' />
    </svg>
  );
}

function Seal({ phrase }: { phrase: string }) {
  const id = useId().replace(/[^a-zA-Z0-9]/g, '');
  const ringId = `seal-ring-${id}`;
  const arcId = `seal-arc-${id}`;
  const gradId = `seal-ink-${id}`;
  const cjk = hasCjk(phrase);

  return (
    <svg className={styles.seal} viewBox='0 0 120 120' aria-hidden='true'>
      <defs>
        <linearGradient id={gradId} x1='8' y1='8' x2='112' y2='112' gradientUnits='userSpaceOnUse'>
          <stop offset='0%' stopColor='#7ea6d6' />
          <stop offset='48%' stopColor='#c5b4c8' />
          <stop offset='100%' stopColor='#e2ae86' />
        </linearGradient>
        <path id={ringId} d='M60 16a44 44 0 1 1 -0.01 0' />
        <path id={arcId} d='M26 66A34 34 0 0 1 94 66' />
      </defs>
      <circle cx='60' cy='60' r='52' fill='none' stroke={`url(#${gradId})`} strokeWidth='1.15' strokeDasharray='1.5 3.2' />
      <circle cx='60' cy='60' r='43.5' fill='none' stroke={`url(#${gradId})`} strokeWidth='0.75' opacity='0.7' />
      <text fill={`url(#${gradId})`} className={cjk ? styles.sealTextCjk : styles.sealText}>
        {cjk ? (
          <textPath href={`#${arcId}`} startOffset='50%' textAnchor='middle'>
            {phrase}
          </textPath>
        ) : (
          <textPath href={`#${ringId}`} textLength='262' lengthAdjust='spacing'>
            {phrase}
          </textPath>
        )}
      </text>
      <circle cx='53.2' cy='60' r='3.15' fill='#7ea4d4' />
      <circle cx='66.8' cy='60' r='3.15' fill='#e4b08a' />
    </svg>
  );
}

function Postmark() {
  return (
    <svg className={styles.postmark} viewBox='0 0 46 70' aria-hidden='true'>
      <path d='M4 14H36M1 24H42M6 34H30M2 44H38M8 54H24' />
    </svg>
  );
}

/** Homepage membership card. Replaces the wireframe wordmark. */
const GuidHomeBrand: React.FC = () => {
  const { t, i18n } = useTranslation();
  const { whoami, status } = useCloudAuth();
  const credits = useContext(CreditsContext);
  const dateLabel = useMemo(() => formatMembershipDate(new Date()), []);

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
    <div className={`${layoutStyles.homeBrand} ${styles.slot}`} data-testid='guid-home-brand'>
      <FlipCard
        axis='y'
        flipOnClick
        draggable
        dragDistance={0}
        tilt
        tiltMax={8}
        glare
        glareOpacity={0.2}
        hoverScale={1.025}
        perspective={1200}
        stiffness={160}
        damping={22}
        width={416}
        height={220}
        radius={24}
        background='transparent'
        color='#17171a'
        shadow
        shadowColor='#7d92b8'
        shadowOpacity={0.2}
        ariaLabel={t('guid.home.cardFlip')}
        front={
          <div className={styles.front}>
            <PlusField />
            <p className={styles.wordmark}>{wordmark}</p>
            <div className={styles.bottomRow}>
              <div className={styles.identity}>
                {checking ? (
                  <span className={styles.nameHold} />
                ) : (
                  <span className={styles.name}>{name}</span>
                )}
                {badge ? <span className={styles.badge}>{badge}</span> : null}
              </div>
              <div className={styles.mark}>
                <span className={styles.date}>{dateLabel}</span>
                <span className={styles.stamp}>
                  <Seal phrase={t('guid.home.cardSeal')} />
                  <Postmark />
                </span>
              </div>
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
  );
};

export default GuidHomeBrand;
