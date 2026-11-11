import React from 'react';
import { BRAND_LOGO_URL, BRAND_NAME } from '@/renderer/config/brand';

type BrandLogoProps = Omit<React.ImgHTMLAttributes<HTMLImageElement>, 'src'> & {
  decorative?: boolean;
};

const BrandLogo: React.FC<BrandLogoProps> = ({ alt, decorative = false, ...props }) => (
  <img src={BRAND_LOGO_URL} alt={decorative ? '' : (alt ?? BRAND_NAME)} {...props} />
);

export default BrandLogo;
