import React from 'react';
import { IconSearch } from '@arco-design/web-react/icon';

type ChromeSearchIconProps = {
  size?: number;
};

/** Shared magnifying-glass used by the titlebar and the conversation header. */
const ChromeSearchIcon: React.FC<ChromeSearchIconProps> = ({ size = 18 }) => (
  <IconSearch className='block leading-none' style={{ fontSize: size }} />
);

export default ChromeSearchIcon;
