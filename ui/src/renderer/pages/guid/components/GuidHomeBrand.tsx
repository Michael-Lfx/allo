import { useContext, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import { useTranslation } from 'react-i18next';
import FlipCard from '@renderer/components/motion/FlipCard';
import FlowyLogo from '@renderer/components/brand/FlowyLogo';
import { formatSiderAccountLabel } from '@renderer/components/layout/Sider/accountLabel';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { CreditsContext } from '@renderer/hooks/context/CreditsContext';
import { membershipDisplayName, shortPlanBadge } from './membershipCardModel';
import layoutStyles from '../index.module.css';
import styles from './GuidHomeCard.module.css';

const hasCjk = (value: string) => /[\u3400-\u9fff]/.test(value);

/** Hover preview is disabled for reduced motion and non-fine pointers. */
function usePrefersReducedMotion() {
  const [reduce, setReduce] = useState(false);
  useEffect(() => {
    const query = window.matchMedia?.('(prefers-reduced-motion: reduce)');
    if (!query) return;
    const apply = () => setReduce(query.matches);
    apply();
    query.addEventListener?.('change', apply);
    return () => query.removeEventListener?.('change', apply);
  }, []);
  return reduce;
}

/** Homepage membership card. Compact cross-two-rows flip card. */
const GuidHomeBrand: React.FC = () => {
  const { t, i18n, } = useTranslation();
  const { whoami, status } = useCloudAuth();
  const credits = useContext(CreditsContext);
  const slotRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 416, height: 220 });
  // Hover previews the back face after ~100ms; a click/keyboard activation pins
  // it explicitly. Reduced motion keeps only click/keyboard flips.
  const reduceMotion = usePrefersReducedMotion();
  const [preview, setPreview] = useState(false);
  const [pinned, setPinned] = useState(false);
  // After an explicit unpin click, do not let the hover preview immediately
  // flip the card back while the pointer is still resting on it.
  const suppressPreviewRef = useRef(false);
  const hoverTimerRef = useRef<number | null>(null);
  const clearHoverTimer = () => {
    if (hoverTimerRef.current !== null) {
      window.clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
  };
  useEffect(() => clearHoverTimer, []);
  const handlePointerEnter = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (reduceMotion || event.pointerType !== 'mouse' || suppressPreviewRef.current) return;
    clearHoverTimer();
    hoverTimerRef.current = window.setTimeout(() => {
      hoverTimerRef.current = null;
      setPreview(true);
    }, 100);
  };
  const handlePointerLeave = () => {
    clearHoverTimer();
    suppressPreviewRef.current = false;
    setPreview(false);
  };
  const showBack = pinned || preview;

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
  const planDisplay = signedIn ? planLabel || badge : '';

  return (
    <div
      className={`${layoutStyles.homeBrand} ${styles.slot}`}
      ref={slotRef}
      data-testid='guid-home-brand'
      onPointerEnter={handlePointerEnter}
      onPointerLeave={handlePointerLeave}
      onClick={(event) => {
        // detail === 0 is keyboard/assistive activation; FlipCard's own onClick
        // already flips and pins it. Mouse activation is handled here exactly once.
        if (event.detail === 0) return;
        if ((event.target as HTMLElement | null)?.closest('a,button')) return;
        setPinned((current) => (preview && !current ? true : !current));
        setPreview(false);
        suppressPreviewRef.current = false;
      }}
    >
      <div className={styles.cardFill}>
        <FlipCard
          axis='y'
          flipped={showBack}
          onFlipChange={(next) => {
            // Mouse activation is handled by the wrapper click below (flipOnClick
            // is off), so this callback is the keyboard path: it pins the face the
            // user flipped to, keeping it after pointer leave.
            setPinned(next);
            setPreview(false);
          }}
          flipOnClick={false}
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
              <div className={styles.memberFace}>
                <FlowyLogo size={44} title='Flowy' />
                {checking ? (
                  <span className={styles.memberNameHold} />
                ) : (
                  <span className={styles.memberName} title={name}>
                    {name}
                  </span>
                )}
                {planDisplay ? <span className={styles.memberPlan}>{planDisplay}</span> : null}
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
