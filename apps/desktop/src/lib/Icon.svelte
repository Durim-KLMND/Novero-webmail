<script lang="ts" module>
  // Every icon in the app comes from here, so they share one style: line
  // icons (Lucide, ISC licence) drawn in the colour of the text around them,
  // which follows the light or dark theme.
  import Archive from "@lucide/svelte/icons/archive";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Bell from "@lucide/svelte/icons/bell";
  import Bold from "@lucide/svelte/icons/bold";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import Check from "@lucide/svelte/icons/check";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import ClipboardPaste from "@lucide/svelte/icons/clipboard-paste";
  import Copy from "@lucide/svelte/icons/copy";
  import Clock from "@lucide/svelte/icons/clock";
  import Cloud from "@lucide/svelte/icons/cloud";
  import Lock from "@lucide/svelte/icons/lock";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import FilePen from "@lucide/svelte/icons/file-pen";
  import FileText from "@lucide/svelte/icons/file-text";
  import Flag from "@lucide/svelte/icons/flag";
  import FlagOff from "@lucide/svelte/icons/flag-off";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import Forward from "@lucide/svelte/icons/forward";
  import Image from "@lucide/svelte/icons/image";
  import Inbox from "@lucide/svelte/icons/inbox";
  import Italic from "@lucide/svelte/icons/italic";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import Link from "@lucide/svelte/icons/link";
  import List from "@lucide/svelte/icons/list";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import ListOrdered from "@lucide/svelte/icons/list-ordered";
  import LogIn from "@lucide/svelte/icons/log-in";
  import Maximize from "@lucide/svelte/icons/square";
  import MapPin from "@lucide/svelte/icons/map-pin";
  import Mail from "@lucide/svelte/icons/mail";
  import MailOpen from "@lucide/svelte/icons/mail-open";
  import Mails from "@lucide/svelte/icons/mails";
  import Palette from "@lucide/svelte/icons/palette";
  import Play from "@lucide/svelte/icons/play";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import PenLine from "@lucide/svelte/icons/pen-line";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Power from "@lucide/svelte/icons/power";
  import Minus from "@lucide/svelte/icons/minus";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import RemoveFormatting from "@lucide/svelte/icons/remove-formatting";
  import Repeat from "@lucide/svelte/icons/repeat";
  import Reply from "@lucide/svelte/icons/reply";
  import ReplyAll from "@lucide/svelte/icons/reply-all";
  import Scissors from "@lucide/svelte/icons/scissors";
  import Search from "@lucide/svelte/icons/search";
  import Send from "@lucide/svelte/icons/send";
  import Settings from "@lucide/svelte/icons/settings";
  import ShieldAlert from "@lucide/svelte/icons/shield-alert";
  import TextCursor from "@lucide/svelte/icons/text-cursor";
  import Trash from "@lucide/svelte/icons/trash";
  import Underline from "@lucide/svelte/icons/underline";
  import UserRound from "@lucide/svelte/icons/user-round";
  import UserRoundMinus from "@lucide/svelte/icons/user-round-minus";
  import Users from "@lucide/svelte/icons/users";
  import X from "@lucide/svelte/icons/x";

  const icons = {
    archive: Archive,
    "move-down": ArrowDown,
    "move-up": ArrowUp,
    bell: Bell,
    bold: Bold,
    calendar: CalendarDays,
    check: Check,
    "chevron-left": ChevronLeft,
    "chevron-down": ChevronDown,
    "chevron-right": ChevronRight,
    alert: CircleAlert,
    paste: ClipboardPaste,
    copy: Copy,
    clock: Clock,
    more: Ellipsis,
    "external-link": ExternalLink,
    drafts: FilePen,
    flag: Flag,
    unflag: FlagOff,
    folder: Folder,
    move: FolderInput,
    forward: Forward,
    image: Image,
    inbox: Inbox,
    italic: Italic,
    keyboard: Keyboard,
    link: Link,
    list: List,
    rules: ListFilter,
    "list-ordered": ListOrdered,
    "sign-in": LogIn,
    location: MapPin,
    lock: Lock,
    cloud: Cloud,
    unread: Mail,
    mail: Mail,
    read: MailOpen,
    "all-inboxes": Mails,
    palette: Palette,
    attachment: Paperclip,
    signature: PenLine,
    template: FileText,
    edit: Pencil,
    power: Power,
    plus: Plus,
    minimize: Minus,
    maximize: Maximize,
    restore: Copy,
    run: Play,
    refresh: RefreshCw,
    "clear-formatting": RemoveFormatting,
    repeat: Repeat,
    reply: Reply,
    "reply-all": ReplyAll,
    cut: Scissors,
    search: Search,
    sent: Send,
    settings: Settings,
    junk: ShieldAlert,
    "select-all": TextCursor,
    trash: Trash,
    underline: Underline,
    contact: UserRound,
    "remove-account": UserRoundMinus,
    contacts: Users,
    close: X,
  };

  export type IconName = keyof typeof icons;
</script>

<script lang="ts">
  interface Props {
    name: IconName;
    size?: number;
    /** Filled in, as for a set flag. */
    filled?: boolean;
    /** Read out instead of being skipped by screen readers. */
    label?: string;
  }
  let { name, size = 16, filled = false, label }: Props = $props();
  const Component = $derived(icons[name]);
</script>

<Component
  {size}
  strokeWidth={1.75}
  fill={filled ? "currentColor" : "none"}
  class="icon"
  aria-hidden={label ? undefined : "true"}
  aria-label={label}
  role={label ? "img" : undefined}
/>
