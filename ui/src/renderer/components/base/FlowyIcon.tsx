/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import {
  Alarm,
  ArrowCircleLeft,
  ArrowClockwise,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Book,
  BookOpen,
  Browser,
  CaretDown,
  CaretRight,
  ChatCircle,
  ChatText,
  Check,
  CheckCircle,
  Cloud,
  Copy,
  Crosshair,
  DeviceMobile,
  DownloadSimple,
  Flask,
  Folder,
  FolderOpen,
  FolderPlus,
  Gear,
  Ghost,
  Globe,
  GraduationCap,
  Headset,
  Images,
  Info,
  Key,
  Keyboard,
  Lightning,
  ListBullets,
  ListChecks,
  MagnifyingGlass,
  Minus,
  Palette,
  Paperclip,
  PencilSimple,
  Phone,
  PhoneDisconnect,
  Plus,
  PuzzlePiece,
  Quotes,
  Robot,
  Shield,
  SidebarSimple,
  SignOut,
  SlidersHorizontal,
  Spinner,
  Swatches,
  Translate,
  Trash,
  User,
  Users,
  VideoCamera,
  Warning,
  Waveform,
  Wrench,
  X,
  type Icon,
  type IconWeight,
} from '@phosphor-icons/react';
import classNames from 'classnames';

const GLYPHS = {
  aiming: Crosshair,
  alarm: Alarm,
  arrowCircleLeft: ArrowCircleLeft,
  arrowLeft: ArrowLeft,
  arrowRight: ArrowRight,
  arrowUp: ArrowUp,
  attention: Warning,
  book: Book,
  bookOpen: BookOpen,
  caution: Warning,
  check: Check,
  checkCircle: CheckCircle,
  close: X,
  cloud: Cloud,
  config: SlidersHorizontal,
  copy: Copy,
  delete: Trash,
  down: CaretDown,
  download: DownloadSimple,
  edit: PencilSimple,
  expandRight: SidebarSimple,
  experiment: Flask,
  folder: Folder,
  folderOpen: FolderOpen,
  folderPlus: FolderPlus,
  ghost: Ghost,
  globe: Globe,
  graduationCap: GraduationCap,
  headset: Headset,
  images: Images,
  info: Info,
  key: Key,
  keyboard: Keyboard,
  lightning: Lightning,
  list: ListBullets,
  listChecks: ListChecks,
  loading: Spinner,
  message: ChatCircle,
  messageText: ChatText,
  minus: Minus,
  mobile: DeviceMobile,
  palette: Palette,
  paperclip: Paperclip,
  phone: Phone,
  phoneOff: PhoneDisconnect,
  plus: Plus,
  puzzle: PuzzlePiece,
  quotes: Quotes,
  refresh: ArrowClockwise,
  right: CaretRight,
  robot: Robot,
  search: MagnifyingGlass,
  settings: Gear,
  shield: Shield,
  signOut: SignOut,
  spinner: Spinner,
  swatches: Swatches,
  tool: Wrench,
  translate: Translate,
  user: User,
  users: Users,
  video: VideoCamera,
  voice: Waveform,
  warning: Warning,
  web: Browser,
} as const satisfies Record<string, Icon>;

export type FlowyIconName = keyof typeof GLYPHS;

const ICON_SIZES = [12, 14, 16, 20] as const;
export type FlowyIconSize = (typeof ICON_SIZES)[number];

/** Snap arbitrary call-site sizes onto the 12 / 14 / 16 / 20 optical grid. */
export function snapFlowyIconSize(size: number): FlowyIconSize {
  if (size <= 12) return 12;
  if (size <= 14) return 14;
  if (size <= 16) return 16;
  return 20;
}

type FlowyIconProps = {
  name: FlowyIconName;
  /** Optical size. Values outside 12 / 14 / 16 / 20 snap to that grid. 14 is the search glyph. */
  size?: number;
  /** `fill` is only for a selected navigation state or an icon sitting on a filled action. */
  weight?: Extract<IconWeight, 'regular' | 'bold' | 'fill'>;
  className?: string;
};

/**
 * Product icon. Chrome uses Phosphor regular at 16px so the stroke matches
 * 13px interface text. Do not import IconPark or Lucide for new controls.
 */
export function FlowyIcon({ name, size = 16, weight = 'regular', className }: FlowyIconProps) {
  const Glyph = GLYPHS[name];
  return (
    <Glyph
      size={snapFlowyIconSize(size)}
      weight={weight}
      color='currentColor'
      className={classNames('flowy-icon', className)}
      aria-hidden
    />
  );
}
