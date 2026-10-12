import React from 'react';
import { useTranslation } from 'react-i18next';
import { planBenefitRows, type BillingPlan } from './billingCatalog';

export default function BillingPlanBenefits({ plan, locale }: { plan: BillingPlan; locale: string }) {
  const { t } = useTranslation();
  const rows = planBenefitRows(plan, locale);
  const expiries = rows.flatMap((row) => row.expiry ? [row.expiry] : []);
  const commonExpiry = expiries.length > 1 && new Set(expiries).size === 1 ? expiries[0] : undefined;
  return (
    <>
      <ul className='billing-plan-benefits'>
        {rows.map((benefit) => (
          <li key={benefit.text}>
            <span>{benefit.text.split(/(\d{1,3}(?:,\d{3})+)/).map((part, index) =>
              /^\d{1,3}(?:,\d{3})+$/.test(part)
                ? <strong className='billing-benefit-value' key={index}>{part}</strong>
                : part
            )}</span>
            {benefit.expiry && !commonExpiry && <small>{benefit.expiry}</small>}
          </li>
        ))}
      </ul>
      {commonExpiry && <p className='billing-plan-validity'>{t('billing.panel.creditValidity', { validity: commonExpiry })}</p>}
    </>
  );
}
