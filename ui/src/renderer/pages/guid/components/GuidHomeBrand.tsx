import TechText from '@renderer/components/layout/Sider/TechText';
import React from 'react';
import styles from '../index.module.css';

/** Homepage wordmark. Animated face, without the logo mark. */
const GuidHomeBrand: React.FC = () => {
  return (
    <div className={styles.homeBrand} data-testid='guid-home-brand'>
      <div className={styles.homeBrandWord}>
        <TechText
          text='Flowy'
          fontWeight={500}
          fontSize={52}
          letterSpacing={0.04}
          align='left'
          reveal='letter'
          dashLength={4}
          dashGap={2}
          strokeWidth={1.5}
          specks={0}
          selection={false}
          labels={false}
          draggable={false}
          clickKick={false}
          intro='none'
          guides={false}
          sweep
          speed={0.55}
          style={{ width: '100%', height: '100%' }}
        />
      </div>
    </div>
  );
};

export default GuidHomeBrand;
