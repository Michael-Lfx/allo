import React from 'react';

const BAR_COUNT = 6;

type MeetingWaveformProps = {
  active: boolean;
};

const MeetingWaveform: React.FC<MeetingWaveformProps> = ({ active }) => (
  <div className='flex h-18px items-end gap-2px' aria-hidden>
    {Array.from({ length: BAR_COUNT }).map((_, index) => (
      <span
        key={index}
        className={
          active
            ? 'meeting-waveform-bar meeting-waveform-bar--live w-2px rounded-full bg-white/90'
            : 'w-2px rounded-full bg-white/35 h-6px'
        }
        style={active ? { animationDelay: `${index * 90}ms` } : undefined}
      />
    ))}
  </div>
);

export default MeetingWaveform;
