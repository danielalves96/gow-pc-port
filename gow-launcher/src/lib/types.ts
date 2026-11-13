// Shapes exchanged with the Rust side (`gow-core`, serde camelCase). Keep in sync with the crate.

export type Language = 'en' | 'pt-BR';

export interface Settings {
  language: string;
  gpu: string;
  displayMode: 'fullscreen' | 'window' | string;
  widthPx: number;
  heightPx: number;
  aspectMode: string;
  /** Widescreen switch: the PCSX2 patch widens the camera (Hor+) at `aspectMode`; off keeps the original 4:3 picture. */
  widescreen: boolean;
  stretch: boolean;
  filter: 'fsr' | 'bilinear' | 'pixel' | string;
  sharpness: number;
  fxaa: boolean;
  /** Shade Boost, 0..100 with 50 = no change (`PC_BRIGHTNESS`, `PC_CONTRAST`, `PC_GAMMA`, `PC_SATURATION`). */
  brightness: number;
  contrast: number;
  gamma: number;
  saturation: number;
  progressive480p: boolean;
  internalResolution: string;
  fpsLimit: number;
  vsync: boolean;
  /** In-game OSD, one switch per part: FPS (`PC_SHOW_FPS`), GPU (`PC_SHOW_GPU`), CPU (`PC_SHOW_CPU`), RAM (`PC_SHOW_RAM`). */
  showFps: boolean;
  showGpu: boolean;
  showCpu: boolean;
  showRam: boolean;
  earlyFinish: boolean;
  /** Effect switches for weak GPUs: character shadows (`GOW_FX_SHADOWS`), additive 3D effects (`GOW_FX_ADDITIVE`), texture filtering (`GOW_FX_FILTER`). */
  fxShadows: boolean;
  fxAdditive: boolean;
  fxFilter: boolean;
  elf: string;
  iso: string;
  executable: string;
  skipVideos: boolean;
  fastBoot: boolean;
  customTextures: boolean;
  asyncTextures: boolean;
  settingsVersion: number;
  dmc5Controls: boolean;
  padButtons: string;
  /** `source=target;...` for every gamepad button (the game's `PC_PAD_MAP`); empty = the layout and face buttons above. */
  padMap: string;
  /** GUID of the joystick the game reads (`GOW_PAD_DEVICE`); empty = the game picks the pad itself. */
  padDevice: string;
  /** Its name, only to show it while it is not connected. */
  padDeviceName: string;
  /** SDL mapping fields for that device when the system has none (`a:b0,start:b7,...`); empty = none. */
  padSdlMap: string;
  cancelAnimations: boolean;
  escQuits: boolean;
  minimizeOnPlay: boolean;
  /** Opening the launcher opens the game without the window (Shift or `--launcher` shows it). */
  skipLauncher: boolean;
  secondsPlayed: number;
  lastSession: number;
  keys: string;
  cheats: string;
  cheatsShortcut: string;
  retroAchievementsEnabled: boolean;
  retroAchievementsUser: string;
  retroAchievementsHardcore: boolean;
  retroAchievementsNotifications: boolean;
  optimizationDestination: string;
  optimizationEnabled: boolean;
  optimizeDuringGame: boolean;
  optimizationCompiled: number;
  /** Install a newer launcher at start-up when nothing is running (off: only the notice). */
  autoUpdate: boolean;
}

/** A joystick as the game listed it the last time it ran (`gow_core::controller::PadDevice`). */
export interface PadDevice {
  /** GLFW's joystick id; the game only reads 0 to 3. */
  index: number;
  name: string;
  guid: string;
  /** GLFW has an SDL mapping for it; without one the game sees every button released. */
  mapped: boolean;
  buttons: number;
  hats: number;
  axes: number;
}

/** The list the game wrote (`gow_core::controller::PadDeviceList`). */
export interface PadDeviceList {
  /** What the game did with the device it was asked for: `automatic`, `found`, `absent` or `outOfRange`. */
  choice: string;
  /** The joystick the game read when it wrote the list (-1: none). */
  active: number;
  devices: PadDevice[];
  /** Unix seconds of the last change of the list. */
  seenAt: number;
}

/** What one joystick does right now, from the game (`gow_core::controller::LiveDevice`): the raw inputs held or moved off rest. */
export interface LiveDevice {
  index: number;
  guid: string;
  /** Raw buttons held. */
  buttons: number[];
  /** `[hat, direction bits]` for each hat off rest. */
  hats: Array<[number, number]>;
  /** `[axis, distance from rest in hundredths]` for each axis that moved. */
  axes: Array<[number, number]>;
}

export interface LiveState {
  /** Grows with every write: a changing beat means the game is alive. */
  beat: number;
  devices: LiveDevice[];
  /** Milliseconds since the game wrote it. */
  ageMs: number;
}

export interface DisplayInfo {
  widthPx: number;
  heightPx: number;
  hz: number;
}

export interface GraphicsCard {
  id: string;
  kind: string;
  displayName: string;
  manufacturer: string;
  driver: string;
  isDefault: boolean;
}

export interface Bootstrap {
  settings: Settings;
  version: string;
  platform: 'windows' | 'linux';
  display: DisplayInfo;
  graphicsCards: GraphicsCard[];
  dataDirectory: string;
  arguments: { play: boolean; wizard: boolean };
  /** The launcher's own executable (for Steam's "Add a Non-Steam Game"). */
  launcherPath: string;
  /** Why the window opened after a direct start (issue #40): the game failed to start, or started and closed with an error. */
  notice: StartNotice | null;
}

export interface StartNotice {
  finished: GameFinished | null;
  launchError: LaunchError | null;
}

export interface GameState {
  exists: boolean;
  valid: boolean;
  modified: boolean;
  crc: string;
}
export interface DataState {
  ok: boolean;
  iso: string;
  extracted: boolean;
  chosenIsoMissing: boolean;
}
export interface ExecutableState {
  exists: boolean;
  executable: boolean;
}
/** One save of the memory card (or of a backup), as the game lists it. */
export interface CardSlot {
  index: number;
  file: string;
  /** Seconds since 1970 (UTC). */
  modifiedUnix: number;
  /** The zone code the save stands in (`Athn02C`). */
  zone: string;
  /** Play time in seconds. */
  playSeconds: number;
  /** The altar the save was made at (0 when unknown): names the place and the thumbnail in `SaveArt`. */
  savePoint: number;
  /** 0 to 3, easiest first. */
  difficulty: number;
}

/** Names and thumbnails from the player's installed disc (`gow_core::save_art::SaveArt`); empty without it. */
export interface SaveArt {
  /** Save point -> name, as the game writes it. */
  places: Record<string, string>;
  /** Difficulty 0..3 -> name. */
  difficulties: string[];
  /** Save point -> `data:image/png;base64,...`. */
  thumbnails: Record<string, string>;
}

/** A dated copy of the card the game made at an altar, or the player asked for. */
export interface SaveBackup {
  id: string;
  created: string;
  createdUnix: number;
  /** `auto`, `manual` or `before-restore`. */
  source: string;
  size: number;
}

export interface GameStatus {
  game: GameState;
  data: DataState;
  executable: ExecutableState;
  ready: boolean;
}

export interface GameFinished {
  code: number | null;
  failed: boolean;
  seconds: number;
  summary: string;
  /** The i18n key that explains a recognized failure (`failureOpenGl`...), shown above the raw summary; null if unknown. */
  failureKind: string | null;
}

export interface LaunchError {
  key: string;
  detail: string;
}

export type OptimizerState = 'disabled' | 'idle' | 'compiling' | 'error';
export interface OptimizerSnapshot {
  configured: boolean;
  compiling: boolean;
  captured: number;
  compiled: number;
  completionRatio: number;
  buildProgress: number;
  status: OptimizerState;
  /** The run in progress is the speed-profile optimization (the game opens by itself). */
  profiling: boolean;
  /** The installation has its speed profile (PGO), trained on this PC. */
  profileReady: boolean;
  /** This installation can have one (Windows, built with clang). */
  profileSupported: boolean;
  /** Where "Optimize now" is: the instrumented build, the game open by itself recording, the build that uses the profile. */
  profileStage: ProfileStage;
  /**
   * The installation holds the instrumented training build of an interrupted "Optimize now": several times slower than the
   * game, so PLAY is refused and the optimizer rebuilds the game by itself.
   */
  instrumented: boolean;
  /** What the build in progress is doing, for the details window (empty while nothing runs). */
  activity: BuildActivity;
}

export type ProfileStage = 'none' | 'building' | 'training' | 'finishing';

/**
 * What a build in progress is doing (issue #32). Times are milliseconds since the Unix epoch on this PC, 0 = not yet.
 * Ninja prints a file only when it is done, so a long step (native VU1 code, the link) prints nothing for minutes:
 * `cpuPercent` (share of one core used by the build's processes over the last second, -1 unknown) is the sign of life.
 */
export interface BuildActivity {
  step: string;
  filesDone: number;
  filesTotal: number;
  lastFinished: string;
  recentLines: string[];
  startedAtMs: number;
  lastOutputMs: number;
  cpuPercent: number;
  lastBusyMs: number;
}

export interface WizardInfo {
  environment: {
    vs: boolean;
    /** Windows: there is a Visual Studio but no usable CMake (3.21 or newer, with cmcldeps.exe) in PATH or bundled with it.
     * Informational: the installer downloads a portable CMake in that case. */
    cmakeUnusable: boolean;
    git: boolean;
    installer: boolean;
    /** Linux: programs and libraries the build needs and the system lacks (empty on Windows or when all is there). */
    missing: string[];
    /** Linux: the packages that provide them, for this distribution (empty if it is not a known one). */
    missingPackages: string[];
    /** Linux: the command that installs those packages, to run in a terminal. */
    installCommand: string;
    /** Windows x86-64: CPU features the game needs that this processor lacks (`['SSE4.1']`); empty when all are there. */
    cpuMissing: string[];
    /** Installation kit files (relative to the kit folder) that are gone and could not be restored; empty when all are there. */
    kitMissing: string[];
    /** Windows: the kit has the prebuilt Rust tools, so the clang build needs no Visual Studio or Git (0.3.0). */
    portableToolchain?: boolean;
  };
  hasManifest: boolean;
  defaultDestination: string;
}

export type IsoReason = 'empty' | 'not-found' | 'unreadable' | 'format' | 'other-game' | 'no-executable' | 'modified' | 'other-edition' | 'chd';
export interface IsoValidation {
  ok: boolean;
  reason: IsoReason | null;
  hash: string;
  /** Accepted as a translation: the USA code (.text) without the original disc's executable. RetroAchievements stay off. */
  translation?: boolean;
  /** With 'other-edition': which God of War edition it is ("Japan", "Europe / Australia"...). */
  edition?: string;
}

/** Why a translation patch could not be applied (`gow_core::iso_patch::PatchError`). */
export type PatchError =
  | 'patch-unreadable' | 'not-xdelta' | 'compressed' | 'wrong-iso' | 'damaged' | 'space' | 'changes-code' | 'cancelled' | 'write';

/** What applying a translation patch produced (`gow_core::iso_patch::PatchOutcome`). */
export interface PatchOutcome {
  ok: boolean;
  /** The translated ISO, next to the original. */
  path: string;
  validation: IsoValidation | null;
  error: PatchError | null;
  /** The decoder's own message, in English. */
  detail: string;
}

export type DestinationReason = 'empty' | 'characters' | 'space';
export interface DestinationValidation {
  ok: boolean;
  reason: DestinationReason | null;
  freeGb: number;
}

export interface InstallProgress {
  stage: number;
  stageProgress: number;
  recentLines: string[];
  /** What the installation is doing, for the wizard's details window (issue #32). */
  activity: BuildActivity;
}
export interface InstallOutcome {
  ok: boolean;
  cancelled: boolean;
  error: string;
  elf: string;
  executable: string;
  /** The i18n key that explains a recognized failure (`failureDownload`...), shown above the raw error; null if unknown. */
  failureKind: string | null;
}

export interface GamepadCaptured {
  binding: string;
  label: string;
}

/** Size, ETag and upload date of the community texture pack (its `_meta` endpoint). */
export interface TexturePackMeta {
  size: number;
  etag: string;
  uploaded: string;
}

/** What the texture page reads: the pack online and what is installed now. */
export interface TexturePackInfo {
  meta: TexturePackMeta | null;
  status: TexturePackStatus;
}

export interface TexturePackStatus {
  installed: boolean;
  files: number;
  bytes: number;
  etag: string;
  uploaded: string;
}

export interface TexturePackProgress {
  stage: 'downloading' | 'extracting';
  /** Bytes downloaded, or files extracted. */
  done: number;
  /** Content length, or number of files to extract (0 when unknown). */
  total: number;
  /** Progress of the whole installation, 0 to 1: the download first, then the extraction, one bar that never goes back. */
  overall: number;
}

export interface TexturePackOutcome {
  ok: boolean;
  status: TexturePackStatus | null;
  /** English detail, for the log/dialog body. */
  error: string | null;
  /** Stable code the interface translates (`noGame`, `noSpace`, `network`...). */
  code: string | null;
}

/** Nothing to show (no newer release, or the check failed), a release to offer, or the update running or failed. */
export type UpdatePhase = 'idle' | 'available' | 'downloading' | 'installing' | 'error';

/** What the button does: update in place, or open the release page (the portable zip, a package install, an unsigned release). */
export type UpdateMode = 'automatic' | 'portable' | 'manual';

/** Event `update-state` and command `update_state`. */
export interface UpdateState {
  phase: UpdatePhase;
  mode: UpdateMode;
  version: string;
  pageUrl: string;
  /** Bytes downloaded and announced size (0 while unknown). */
  done: number;
  total: number;
  /** English detail of the last failure, for the tooltip. */
  error: string;
}

/** Whether the game build is older than the runtime sources the launcher carries (`gow_core::rebuild::RebuildState`). */
export interface RebuildState {
  needed: boolean;
  /** Short digests of the kit and of what the build was made from, for the log. */
  kit: string;
  built: string;
  /** True the first time this digest asks for a rebuild: the interface shows a toast once. */
  announce: boolean;
  /** The build was made by 0.2.9 or older (Visual Studio): the rebuild deletes it and builds from scratch. */
  legacy?: boolean;
}

/** What an update must not interrupt; the backend refuses with `busy:<code>` for the same reasons. */
export type UpdateBlocker = 'game' | 'installation' | 'optimizer' | 'texturePack';

/** Why a CHD could not be converted to ISO (`gow_core::iso_chd::ChdError`). */
export type ChdError = 'unreadable' | 'damaged' | 'space' | 'not-game' | 'cancelled' | 'write';

/** What converting a CHD will do (`gow_core::iso_chd::ChdPlan`). */
export interface ChdPlan {
  /** The ISO that will be written next to the CHD, or the one already converted there. */
  output: string;
  isoGb: number;
  freeGb: number;
  enoughSpace: boolean;
  /** Converted before and valid: it is used as it is. */
  existing: boolean;
}

/** What converting a CHD produced (`gow_core::iso_chd::ChdOutcome`). */
export interface ChdOutcome {
  ok: boolean;
  /** The ISO, next to the CHD. */
  path: string;
  validation: IsoValidation | null;
  error: ChdError | null;
  /** The reader's own message, in English. */
  detail: string;
}
