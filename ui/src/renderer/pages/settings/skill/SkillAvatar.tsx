import { getBaseUrl } from '@/common/adapter/httpBridge';
import type { SkillInfo } from '@/renderer/pages/settings/PresetSettings/types';
import { Lightning } from '@icon-park/react';
import React, { useEffect, useState } from 'react';
import { getAvatarColorClass, resolveSkillAvatarSrc } from './skillPresentation';

type SkillAvatarProps = {
  skill: Pick<SkillInfo, 'name' | 'avatar'>;
  isAutoInjected?: boolean;
  size?: number;
  radiusClassName?: string;
  showShadow?: boolean;
};

const SkillAvatar: React.FC<SkillAvatarProps> = ({
  skill,
  isAutoInjected = false,
  size = 36,
  radiusClassName = 'rounded-10px',
  showShadow = true,
}) => {
  const [broken, setBroken] = useState(false);
  const src = broken ? undefined : resolveSkillAvatarSrc(skill.avatar, getBaseUrl());

  useEffect(() => {
    setBroken(false);
  }, [skill.avatar]);

  if (src) {
    return (
      <span
        className={`inline-flex flex-shrink-0 overflow-hidden ${radiusClassName} bg-[var(--color-fill-2)] ${showShadow ? 'shadow-sm' : ''}`}
        style={{ width: size, height: size }}
      >
        <img
          src={src}
          alt=''
          width={size}
          height={size}
          className='h-full w-full object-cover'
          onError={() => setBroken(true)}
        />
      </span>
    );
  }

  if (isAutoInjected) {
    return (
      <div
        className={`flex flex-shrink-0 items-center justify-center ${radiusClassName} bg-[rgba(var(--success-6),0.1)] ${showShadow ? 'shadow-sm' : ''}`}
        style={{ width: size, height: size }}
      >
        <Lightning theme='filled' size={Math.round(size * 0.5)} fill='rgb(var(--success-6))' />
      </div>
    );
  }

  return (
    <div
      className={`flex flex-shrink-0 items-center justify-center font-bold uppercase ${showShadow ? 'shadow-sm' : ''} ${radiusClassName} ${getAvatarColorClass(skill.name)}`}
      style={{ width: size, height: size, fontSize: Math.round(size * 0.42) }}
    >
      {skill.name.charAt(0).toUpperCase()}
    </div>
  );
};

export default SkillAvatar;
