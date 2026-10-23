import type { ComponentType, ComponentPropsWithoutRef, ReactNode } from 'react'
import {
  IconApproved,
  IconArchived,
  IconArrow,
  IconArrowUpRight,
  IconBan,
  IconBarChart,
  IconBraces,
  IconBranch,
  IconBuilding,
  IconCalendar,
  IconCheck,
  IconChevron,
  IconChevrons,
  IconClockArrow,
  IconCode,
  IconCollapsedRow,
  IconComment,
  IconConvo,
  IconDesktop,
  IconExpandRow,
  IconEye,
  IconEyeSlash,
  IconFileText,
  IconFileTree,
  IconFlagFill,
  IconFolder,
  IconFolders,
  IconGear,
  IconGlobe,
  IconImage,
  IconInProgress,
  IconInfo,
  IconCiWarning,
  IconKey,
  IconLayers2,
  IconListCheck,
  IconListUnordered,
  IconMinus,
  IconMoon,
  IconPeople,
  IconPlus,
  IconRefresh,
  IconReload,
  IconRepeat,
  IconReply,
  IconSearch,
  IconSignOut,
  IconSparkles,
  IconSun,
  IconSwitches,
  IconTerminal,
  IconTrash,
  IconWaiting,
  IconWorkflow,
  type IconProps as PierreIconProps,
} from '@pierre/icons'

type IconProps = ComponentPropsWithoutRef<'svg'> & { size?: number | string }

// Preserve native Pierre outlines, proportions and currentColor at every UI scale.
function pierre(Icon: ComponentType<PierreIconProps>, name: string, rotation = 0, mirror = false) {
  return function AppIcon({ size = 16, strokeWidth: _strokeWidth, style, ...props }: IconProps) {
    const transform = [
      rotation && `rotate(${rotation}deg)`,
      mirror && 'scaleX(-1)',
      style?.transform,
    ]
      .filter(Boolean)
      .join(' ')
    return (
      <Icon
        aria-hidden="true"
        {...props}
        size={size}
        data-icon={name}
        style={{ ...style, ...(transform ? { transform } : {}) }}
      />
    )
  }
}
export const AlarmClock = pierre(IconWaiting, 'alarm-clock', 0, false)
export const AlertCircle = pierre(IconCiWarning, 'alert-circle', 0, false)
export const Archive = pierre(IconArchived, 'archive', 0, false)
export const ArchiveRestore = pierre(IconReply, 'archive-restore', 0, false)
export const ArrowDown = pierre(IconArrow, 'arrow-down', -90, false)
export const ArrowLeft = pierre(IconArrow, 'arrow-left', 0, false)
export const ArrowUp = pierre(IconArrow, 'arrow-up', 90, false)
export const ArrowUpRight = pierre(IconArrowUpRight, 'arrow-up-right', 0, false)
export const Ban = pierre(IconBan, 'ban', 0, false)
export const Bot = pierre(IconSparkles, 'bot', 0, false)
export const Braces = pierre(IconBraces, 'braces', 0, false)
export const Building2 = pierre(IconBuilding, 'building2', 0, false)
export const CalendarCheck = pierre(IconCalendar, 'calendar-check', 0, false)
export const Check = pierre(IconCheck, 'check', 0, false)
export const ChevronDown = pierre(IconChevron, 'chevron-down', 0, false)
export const ChevronLeft = pierre(IconChevron, 'chevron-left', 90, false)
export const ChevronRight = pierre(IconChevron, 'chevron-right', -90, false)
export const ChevronsUp = pierre(IconChevrons, 'chevrons-up', 0, false)
export const CircleAlert = pierre(IconCiWarning, 'circle-alert', 0, false)
export const ClipboardCheck = pierre(IconApproved, 'clipboard-check', 0, false)
export const Clock = pierre(IconWaiting, 'clock', 0, false)
export const ExternalLink = pierre(IconArrowUpRight, 'external-link', 0, false)
export const Eye = pierre(IconEye, 'eye', 0, false)
export const EyeOff = pierre(IconEyeSlash, 'eye-off', 0, false)
export const FileText = pierre(IconFileText, 'file-text', 0, false)
export const Flag = pierre(IconFlagFill, 'flag', 0, false)
export const Folder = pierre(IconFolder, 'folder', 0, false)
export const FolderRoot = pierre(IconFolders, 'folder-root', 0, false)
export const Gauge = pierre(IconBarChart, 'gauge', 0, false)
export const GitBranch = pierre(IconBranch, 'git-branch', 0, false)
export const Globe = pierre(IconGlobe, 'globe', 0, false)
export const History = pierre(IconClockArrow, 'history', 0, false)
export const Image = pierre(IconImage, 'image', 0, false)
export const KeyRound = pierre(IconKey, 'key-round', 0, false)
export const Languages = pierre(IconGlobe, 'languages', 0, false)
export const List = pierre(IconListUnordered, 'list', 0, false)
export const ListRestart = pierre(IconRepeat, 'list-restart', 0, false)
export const ListTodo = pierre(IconListCheck, 'list-todo', 0, false)
export const ListTree = pierre(IconFileTree, 'list-tree', 0, false)
export const Loader2 = pierre(IconInProgress, 'loader2', 0, false)
export const LoaderCircle = pierre(IconInProgress, 'loader-circle', 0, false)
export const LogOut = pierre(IconSignOut, 'log-out', 0, false)
export const MessageSquare = pierre(IconComment, 'message-square', 0, false)
export const MessagesSquare = pierre(IconConvo, 'messages-square', 0, false)
export const Minus = pierre(IconMinus, 'minus', 0, false)
export const Monitor = pierre(IconDesktop, 'monitor', 0, false)
export const Moon = pierre(IconMoon, 'moon', 0, false)
export const PanelTopClose = pierre(IconCollapsedRow, 'panel-top-close', 0, false)
export const PanelTopOpen = pierre(IconExpandRow, 'panel-top-open', 0, false)
export const Plus = pierre(IconPlus, 'plus', 0, false)
export const Redo2 = pierre(IconReply, 'redo2', 0, true)
export const RefreshCw = pierre(IconRefresh, 'refresh-cw', 0, false)
export const RotateCw = pierre(IconReload, 'rotate-cw', 0, false)
export const Rss = pierre(IconLayers2, 'rss', 0, false)
export const Search = pierre(IconSearch, 'search', 0, false)
export const Settings2 = pierre(IconGear, 'settings2', 0, false)
export const SlidersHorizontal = pierre(IconSwitches, 'sliders-horizontal', 0, false)
export const Sun = pierre(IconSun, 'sun', 0, false)
export const Terminal = pierre(IconTerminal, 'terminal', 0, false)
export const Trash2 = pierre(IconTrash, 'trash2', 0, false)
export const Undo2 = pierre(IconReply, 'undo2', 0, false)
export const Users = pierre(IconPeople, 'users', 0, false)
export const Workflow = pierre(IconWorkflow, 'workflow', 0, false)
export const Wrench = pierre(IconCode, 'wrench', 0, false)

// These standard controls have no equivalent in Pierre. Match its 1.5px optical
// weight; retain the four-corner expand/shrink shapes already used in Wakeit.
function control(name: string, drawing: ReactNode) {
  return function ControlIcon({ size = 16, ...props }: IconProps) {
    return (
      <svg
        aria-hidden="true"
        width={size}
        height={size}
        viewBox="0 0 16 16"
        fill="none"
        stroke="currentColor"
        strokeWidth={1.5}
        strokeLinecap="round"
        strokeLinejoin="round"
        {...props}
        data-icon={name}
      >
        {drawing}
      </svg>
    )
  }
}
// A flatter single bubble keeps continuation visually level with adjacent actions.
export const ContinuationBubble = control(
  'resume-session',
  <path d="M8 2.75c-3.59 0-6.5 1.96-6.5 4.38 0 1.3.84 2.47 2.2 3.27L2.25 13l4-1.7c.56.13 1.14.2 1.75.2 3.59 0 6.5-1.96 6.5-4.37S11.59 2.75 8 2.75Z" />,
)
export const Play = control('play', <path d="m4 2.5 9 5.5-9 5.5Z" />)
export const Pause = control(
  'pause',
  <>
    <rect x="3.5" y="2.5" width="3" height="11" rx=".5" />
    <rect x="9.5" y="2.5" width="3" height="11" rx=".5" />
  </>,
)
export const Square = control('stop', <rect x="3" y="3" width="10" height="10" rx="1" />)
export const Maximize = control(
  'expand',
  <path d="M6 2H3a1 1 0 0 0-1 1v3m8-4h3a1 1 0 0 1 1 1v3M2 10v3a1 1 0 0 0 1 1h3m8-4v3a1 1 0 0 1-1 1h-3" />,
)
export const Minimize = control(
  'shrink',
  <path d="M2 6h3a1 1 0 0 0 1-1V2m4 0v3a1 1 0 0 0 1 1h3M6 14v-3a1 1 0 0 0-1-1H2m12 0h-3a1 1 0 0 0-1 1v3" />,
)
const zoom = (
  <>
    <circle cx="6.75" cy="6.75" r="5" />
    <path d="m10.5 10.5 3.75 3.75M4.5 6.75H9" />
  </>
)
export const ZoomOut = control('zoom-out', zoom)
export const ZoomIn = control(
  'zoom-in',
  <>
    {zoom}
    <path d="M6.75 4.5V9" />
  </>,
)
