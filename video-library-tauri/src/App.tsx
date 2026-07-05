import { useEffect, useRef, useState } from "react";
import type {
  CSSProperties,
  KeyboardEvent as ReactKeyboardEvent,
  MouseEvent,
  PointerEvent,
  SyntheticEvent,
} from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import "./App.css";

type VideoFile = {
  filename: string;
  file_path: string;
  title: string;
  actor: string;
  genre: string;
  date: string;
  backup_date: string;
  backup_location: string;
  notes: string;
  explicit_content: boolean;
  resolution: string;
  bitrate: string;
  filesize: number;
  artwork_thumbnail: string;
  rating: number;
  play_count: number;
  added_at: string;
};

type VideoDatabase = {
  folder_path?: string;
  folder_paths?: string[];
  videos: VideoFile[];
  actor_thumbnails: Record<string, string>;
  actor_bios: Record<string, string>;
  actor_social_links: Record<string, ActorSocialLinks>;
  watch_statistics: WatchStatistics;
};

type WatchStatistics = {
  total_watches: number;
  actor_counts: Record<string, number>;
  genre_counts: Record<string, number>;
  year_counts: Record<string, number>;
};

type ActorSocialLinks = {
  website: string;
  imdb: string;
  instagram: string;
  x: string;
  youtube: string;
};

type AppSettings = {
  thumbnail_frame_second: string;
  grid_size: string;
  main_view_mode?: MainViewMode;
  font_size?: AppFontSize;
  show_thumbnail_titles?: boolean;
  hide_explicit_content?: boolean;
  explicit_content_password_hash?: string;
  sort_mode?: SortMode;
  secondary_sort_mode?: SecondarySortMode;
};

type ViewMode = "all-videos" | "actors" | "genres" | "ratings" | "settings";
type MainViewMode = "grid" | "list";
type AppFontSize = "small" | "medium" | "large";
type FontSizeVars = CSSProperties & Record<`--${string}`, string>;
type RightPanelMode = "video" | "actor" | "genre" | "rating";
type RightPanelDetailMode = "short" | "extended";
type SortMode =
  | "name"
  | "director"
  | "played-count"
  | "rating"
  | "file-size"
  | "added";
type SecondarySortMode = "none" | SortMode;
type ListSortDirection = "asc" | "desc";
type ListSortKey =
  | "thumbnail"
  | "title"
  | "filename"
  | "actor"
  | "genre"
  | "date"
  | "rating"
  | "play_count"
  | "explicit_content"
  | "filesize"
  | "resolution"
  | "bitrate"
  | "added_at"
  | "backup_date"
  | "backup_location"
  | "notes"
  | "file_path";

type LibraryGroup = {
  name: string;
  videos: VideoFile[];
  artworkVideo: VideoFile;
};

type ContextMenuState = {
  x: number;
  y: number;
  video: VideoFile;
  videos: VideoFile[];
} | null;

type VideoEditForm = {
  title: string;
  actor: string;
  genre: string;
  date: string;
  backup_date: string;
  backup_location: string;
  notes: string;
  explicit_content: "true" | "false" | "various";
  rating: string;
  play_count: string;
};

type AutocompleteField = "actor" | "genre";

type AutocompleteState = {
  field: AutocompleteField;
  activeIndex: number;
} | null;

type NavigationSnapshot = {
  activeView: ViewMode;
  selectedActor: string;
  selectedGenre: string;
  selectedRating: number | null;
  selectedVideoPath: string;
  selectedVideoPaths: string[];
  rightPanelMode: RightPanelMode;
  isRightPanelEditing: boolean;
};

type CropRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type ActorCropState = {
  actorName: string;
  imageSrc: string;
  objectUrl: string;
  crop: CropRect;
};

type RefreshFieldChange = {
  field: string;
  old_value: string;
  new_value: string;
};

type RefreshFileChange = {
  file_path: string;
  filename: string;
  changes: RefreshFieldChange[];
};

type RefreshReport = {
  database: VideoDatabase;
  changed_files: RefreshFileChange[];
  added_files: string[];
  removed_files: string[];
};

type OrganizePreviewItem = {
  file_path: string;
  filename: string;
  target_folder: string;
  target_path: string;
  renamed: boolean;
  error: string;
};

type OrganizePreview = {
  base_folder: string;
  pattern: string;
  folders_to_create: string[];
  items: OrganizePreviewItem[];
};

type OrganizeResult = {
  database: VideoDatabase;
  copied_count: number;
  renamed_count: number;
  updated_count: number;
  errors: string[];
  items: OrganizePreviewItem[];
};

type OrganizeProgress = {
  total: number;
  completed: number;
  current_file: string;
  status: string;
};

type FileOperationProgress = OrganizeProgress;

type BackupResult = {
  database: VideoDatabase;
  zip_path: string;
  backed_up_count: number;
  errors: string[];
};

type RestorePreviewItem = {
  filename: string;
  target_path: string;
  exists: boolean;
  error: string;
};

type RestorePreview = {
  zip_path: string;
  items: RestorePreviewItem[];
};

type RestoreResult = {
  restored_count: number;
  skipped_count: number;
  errors: string[];
  items: RestorePreviewItem[];
};

type FileTagUpdateResult = {
  updated_count: number;
  skipped_count: number;
  errors: string[];
};

type DeleteVideosResult = {
  database: VideoDatabase;
  recycled_count: number;
  removed_from_database_count: number;
  errors: string[];
};

type HoverPreviewState = {
  video: VideoFile;
  x: number;
  y: number;
  showThumbnail: boolean;
};

type VideoTechnicalMetadata = {
  resolution: string;
  bitrate: string;
};

type VideoThumbnailResult = VideoTechnicalMetadata & {
  thumbnailPath: string;
};

const ORGANIZE_PATTERN = "{genre}\\{actor}";

function App() {
  const [selectedFolders, setSelectedFolders] = useState<string[]>([]);
  const [videoFiles, setVideoFiles] = useState<VideoFile[]>([]);
  const [actorThumbnails, setActorThumbnails] = useState<Record<string, string>>(
    {},
  );
  const [actorBios, setActorBios] = useState<Record<string, string>>({});
  const [actorSocialLinks, setActorSocialLinks] = useState<
    Record<string, ActorSocialLinks>
  >({});
  const [errorMessage, setErrorMessage] = useState("");
  const [hasDatabase, setHasDatabase] = useState(false);
  const [selectedVideo, setSelectedVideo] = useState<VideoFile | null>(null);
  const [selectedVideoPaths, setSelectedVideoPaths] = useState<string[]>([]);
  const [activeView, setActiveView] = useState<ViewMode>("actors");
  const [rightPanelMode, setRightPanelMode] = useState<RightPanelMode>("actor");
  const [rightPanelDetailMode, setRightPanelDetailMode] =
    useState<RightPanelDetailMode>("short");
  const [isRightPanelEditing, setIsRightPanelEditing] = useState(false);
  const [databasePath, setDatabasePath] = useState("");
  const [selectedActor, setSelectedActor] = useState("");
  const [selectedGenre, setSelectedGenre] = useState("");
  const [selectedRating, setSelectedRating] = useState<number | null>(null);
  const [filterText, setFilterText] = useState("");
  const [actorFilter, setActorFilter] = useState("all");
  const [genreFilters, setGenreFilters] = useState<string[]>([]);
  const [ratingFilters, setRatingFilters] = useState<number[]>([]);
  const [sortMode, setSortMode] = useState<SortMode>("played-count");
  const [secondarySortMode, setSecondarySortMode] =
    useState<SecondarySortMode>("rating");
  const [listSortKey, setListSortKey] = useState<ListSortKey>("title");
  const [listSortDirection, setListSortDirection] =
    useState<ListSortDirection>("asc");
  const [thumbnailFrameSecond, setThumbnailFrameSecond] = useState("1");
  const [gridSize, setGridSize] = useState("180");
  const [mainViewMode, setMainViewMode] = useState<MainViewMode>("grid");
  const [appFontSize, setAppFontSize] = useState<AppFontSize>("large");
  const [showThumbnailTitles, setShowThumbnailTitles] = useState(true);
  const [hideExplicitContent, setHideExplicitContent] = useState(false);
  const [explicitContentPasswordHash, setExplicitContentPasswordHash] =
    useState("");
  const [contextMenu, setContextMenu] = useState<ContextMenuState>(null);
  const [editForm, setEditForm] = useState<VideoEditForm>({
    title: "",
    actor: "",
    genre: "",
    date: "",
    backup_date: "",
    backup_location: "",
    notes: "",
    explicit_content: "false",
    rating: "0",
    play_count: "0",
  });
  const [autocompleteState, setAutocompleteState] =
    useState<AutocompleteState>(null);
  const [thumbnailFailures, setThumbnailFailures] = useState<Set<string>>(
    () => new Set(),
  );
  const [thumbnailRefreshKeys, setThumbnailRefreshKeys] = useState<
    Record<string, number>
  >({});
  const [actorThumbnailRefreshKeys, setActorThumbnailRefreshKeys] = useState<
    Record<string, number>
  >({});
  const [actorCrop, setActorCrop] = useState<ActorCropState | null>(null);
  const [refreshReport, setRefreshReport] = useState<RefreshReport | null>(null);
  const [isRefreshingDatabase, setIsRefreshingDatabase] = useState(false);
  const [organizePreview, setOrganizePreview] =
    useState<OrganizePreview | null>(null);
  const [organizeResult, setOrganizeResult] = useState<OrganizeResult | null>(
    null,
  );
  const [isOrganizing, setIsOrganizing] = useState(false);
  const [organizeProgress, setOrganizeProgress] =
    useState<OrganizeProgress | null>(null);
  const [isOrganizeProgressMinimized, setIsOrganizeProgressMinimized] =
    useState(false);
  const [watchStatistics, setWatchStatistics] = useState<WatchStatistics>({
    total_watches: 0,
    actor_counts: {},
    genre_counts: {},
    year_counts: {},
  });
  const [isStatisticsOpen, setIsStatisticsOpen] = useState(false);
  const [backupProgress, setBackupProgress] =
    useState<FileOperationProgress | null>(null);
  const [restoreProgress, setRestoreProgress] =
    useState<FileOperationProgress | null>(null);
  const [isBackingUp, setIsBackingUp] = useState(false);
  const [isRestoring, setIsRestoring] = useState(false);
  const [backupResult, setBackupResult] = useState<BackupResult | null>(null);
  const [restorePreview, setRestorePreview] = useState<RestorePreview | null>(
    null,
  );
  const [restoreResult, setRestoreResult] = useState<RestoreResult | null>(null);
  const [pendingDeleteVideos, setPendingDeleteVideos] = useState<VideoFile[]>([]);
  const [pendingUpdate, setPendingUpdate] = useState<Update | null>(null);
  const [updateStatus, setUpdateStatus] = useState("");
  const [updateDownloadProgress, setUpdateDownloadProgress] = useState("");
  const [isCheckingForUpdate, setIsCheckingForUpdate] = useState(false);
  const [isInstallingUpdate, setIsInstallingUpdate] = useState(false);
  const [hoverPreview, setHoverPreview] = useState<HoverPreviewState | null>(
    null,
  );
  const [isLeftPanelCollapsed, setIsLeftPanelCollapsed] = useState(false);
  const [isRightPanelCollapsed, setIsRightPanelCollapsed] = useState(false);
  const isGeneratingThumbnails = useRef(false);
  const settingsLoaded = useRef(false);
  const activeViewRef = useRef<ViewMode>("actors");
  const navigationHistoryRef = useRef<NavigationSnapshot[]>([]);
  const editFormSelectionKeyRef = useRef("");
  const selectionAnchorPathRef = useRef("");
  const hoverPreviewDelayRef = useRef<number | null>(null);
  const hoverThumbnailDelayRef = useRef<number | null>(null);
  const hoverPreviewVideoRef = useRef<VideoFile | null>(null);
  const hoverPreviewPositionRef = useRef({ x: 0, y: 0 });
  const actorCropImageRef = useRef<HTMLImageElement | null>(null);
  const actorCropDragRef = useRef<{
    startX: number;
    startY: number;
    cropX: number;
    cropY: number;
  } | null>(null);

  useEffect(() => {
    async function loadDatabase() {
      try {
        const path = await invoke<string>("database_file_path");
        setDatabasePath(path);

        const settings = await invoke<AppSettings>("load_app_settings");
        setThumbnailFrameSecond(settings.thumbnail_frame_second ?? "1");
        setGridSize(settings.grid_size ?? "180");
        setMainViewMode(settings.main_view_mode ?? "grid");
        setAppFontSize(settings.font_size ?? "large");
        setShowThumbnailTitles(settings.show_thumbnail_titles ?? true);
        setHideExplicitContent(settings.hide_explicit_content ?? false);
        setExplicitContentPasswordHash(
          settings.explicit_content_password_hash ?? "",
        );
        setSortMode(settings.sort_mode ?? "played-count");
        setSecondarySortMode(settings.secondary_sort_mode ?? "rating");
        settingsLoaded.current = true;

        const database = await invoke<VideoDatabase | null>(
          "load_video_database",
        );

        if (database) {
          setSelectedFolders(databaseFolders(database));
          setVideoFiles(database.videos);
          setActorThumbnails(database.actor_thumbnails ?? {});
          setActorBios(database.actor_bios ?? {});
          setActorSocialLinks(database.actor_social_links ?? {});
          setWatchStatistics(normalizeWatchStatistics(database.watch_statistics));
          setSelectedVideo(database.videos[0] ?? null);
          setSelectedVideoPaths(
            database.videos[0] ? [database.videos[0].file_path] : [],
          );
          setRightPanelMode("video");
          setHasDatabase(true);
          setThumbnailFailures(new Set());
          startThumbnailCaching(database.videos);
        }
      } catch (error) {
        settingsLoaded.current = true;
        setErrorMessage(String(error));
      }
    }

    loadDatabase();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    listen<OrganizeProgress>("organize-progress", (event) => {
      setOrganizeProgress(event.payload);
    }).then((eventUnlisten) => {
      unlisten = eventUnlisten;
    });

    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let unlistenBackup: (() => void) | undefined;
    let unlistenRestore: (() => void) | undefined;

    listen<FileOperationProgress>("backup-progress", (event) => {
      setBackupProgress(event.payload);
    }).then((eventUnlisten) => {
      unlistenBackup = eventUnlisten;
    });
    listen<FileOperationProgress>("restore-progress", (event) => {
      setRestoreProgress(event.payload);
    }).then((eventUnlisten) => {
      unlistenRestore = eventUnlisten;
    });

    return () => {
      unlistenBackup?.();
      unlistenRestore?.();
    };
  }, []);

  useEffect(() => {
    if (!settingsLoaded.current) {
      return;
    }

    invoke("save_app_settings", {
      thumbnailFrameSecond,
      gridSize,
      mainViewMode,
      fontSize: appFontSize,
      showThumbnailTitles,
      hideExplicitContent,
      explicitContentPasswordHash,
      sortMode,
      secondarySortMode,
    }).catch((error) => setErrorMessage(String(error)));
  }, [
    thumbnailFrameSecond,
    gridSize,
    mainViewMode,
    appFontSize,
    showThumbnailTitles,
    hideExplicitContent,
    explicitContentPasswordHash,
    sortMode,
    secondarySortMode,
  ]);

  useEffect(() => {
    activeViewRef.current = activeView;
  }, [activeView]);

  useEffect(() => {
    if (!hideExplicitContent) {
      return;
    }

    const visibleVideoList = contentVisibleVideos(videoFiles);
    const visiblePathSet = new Set(
      visibleVideoList.map((video) => video.file_path),
    );

    setSelectedVideoPaths((currentPaths) => {
      const nextPaths = currentPaths.filter((path) => visiblePathSet.has(path));
      return nextPaths.length > 0
        ? nextPaths
        : visibleVideoList[0]
          ? [visibleVideoList[0].file_path]
          : [];
    });
    setSelectedVideo((currentVideo) => {
      if (currentVideo && visiblePathSet.has(currentVideo.file_path)) {
        return currentVideo;
      }

      return visibleVideoList[0] ?? null;
    });
  }, [hideExplicitContent, videoFiles]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") {
        return;
      }

      if (actorCrop) {
        setActorCrop(null);
        return;
      }

      if (refreshReport) {
        setRefreshReport(null);
        return;
      }

      if (isStatisticsOpen) {
        setIsStatisticsOpen(false);
        return;
      }

      if (backupResult) {
        setBackupResult(null);
        return;
      }

      if (restorePreview) {
        setRestorePreview(null);
        return;
      }

      if (restoreResult) {
        setRestoreResult(null);
        return;
      }

      if (pendingDeleteVideos.length > 0) {
        setPendingDeleteVideos([]);
        return;
      }

      if (organizePreview) {
        setOrganizePreview(null);
        return;
      }

      if (organizeResult) {
        setOrganizeResult(null);
        return;
      }

      if (contextMenu) {
        setContextMenu(null);
        return;
      }

      goBackView();
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    actorCrop,
    contextMenu,
    isStatisticsOpen,
    backupResult,
    organizePreview,
    organizeResult,
    pendingDeleteVideos,
    refreshReport,
    restorePreview,
    restoreResult,
  ]);

  useEffect(() => {
    const selectionKey =
      selectedVideoPaths.length > 0
        ? selectedVideoPaths.join("|")
        : selectedVideo?.file_path ?? "";

    if (isRightPanelEditing && editFormSelectionKeyRef.current === selectionKey) {
      return;
    }

    editFormSelectionKeyRef.current = selectionKey;
    const selectedVideos = selectedVideosForEdit();

    if (selectedVideos.length === 0) {
      setEditForm({
        title: "",
        actor: "",
        genre: "",
        date: "",
        backup_date: "",
        backup_location: "",
        notes: "",
        explicit_content: "false",
        rating: "0",
        play_count: "0",
      });
      return;
    }

    setEditForm({
      title: sharedStringValue(selectedVideos, "title"),
      actor: sharedStringValue(selectedVideos, "actor"),
      genre: sharedStringValue(selectedVideos, "genre"),
      date: sharedStringValue(selectedVideos, "date"),
      backup_date: sharedStringValue(selectedVideos, "backup_date"),
      backup_location: sharedStringValue(selectedVideos, "backup_location"),
      notes: sharedStringValue(selectedVideos, "notes"),
      explicit_content: sharedBooleanValue(selectedVideos, "explicit_content"),
      rating: sharedNumberValue(selectedVideos, "rating"),
      play_count: sharedNumberValue(selectedVideos, "play_count"),
    });
  }, [
    isRightPanelEditing,
    selectedVideo?.file_path,
    selectedVideoPaths.join("|"),
    videoFiles,
  ]);

  useEffect(() => {
    function closeContextMenu() {
      setContextMenu(null);
    }

    window.addEventListener("click", closeContextMenu);
    return () => window.removeEventListener("click", closeContextMenu);
  }, []);

  useEffect(() => {
    return () => {
      if (actorCrop?.objectUrl) {
        URL.revokeObjectURL(actorCrop.objectUrl);
      }
    };
  }, [actorCrop?.objectUrl]);

  useEffect(() => {
    return () => {
      clearHoverPreviewTimers();
    };
  }, []);

  async function selectFolder() {
    const folder = await open({
      directory: true,
      multiple: false,
    });

    if (typeof folder === "string") {
      setErrorMessage("");

      try {
        const database = await invoke<VideoDatabase>("scan_mp4_files", {
          folderPath: folder,
        });

        setSelectedFolders(databaseFolders(database));
        setDatabasePath(await invoke<string>("database_file_path"));
        setVideoFiles(database.videos);
        setActorThumbnails(database.actor_thumbnails ?? {});
        setActorBios(database.actor_bios ?? {});
        setActorSocialLinks(database.actor_social_links ?? {});
        setWatchStatistics(normalizeWatchStatistics(database.watch_statistics));
        setSelectedVideo(database.videos[0] ?? null);
        setSelectedVideoPaths(
          database.videos[0] ? [database.videos[0].file_path] : [],
        );
        setRightPanelMode("video");
        setSelectedRating(null);
        setHasDatabase(true);
        setThumbnailFailures(new Set());
        startThumbnailCaching(database.videos);
      } catch (error) {
        setVideoFiles([]);
        setActorThumbnails({});
        setActorBios({});
        setActorSocialLinks({});
        setWatchStatistics(emptyWatchStatistics());
        setSelectedVideo(null);
        setSelectedVideoPaths([]);
        setSelectedRating(null);
        setThumbnailFailures(new Set());
        setErrorMessage(String(error));
      }
    }
  }

  async function exportDatabaseFile() {
    const folder = await open({
      directory: true,
      multiple: false,
    });

    if (typeof folder !== "string") {
      return;
    }

    setErrorMessage("");

    try {
      await invoke<string>("export_database_file", { folderPath: folder });
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  async function addDirectory() {
    const folder = await open({
      directory: true,
      multiple: false,
    });

    if (typeof folder !== "string") {
      return;
    }

    setErrorMessage("");

    try {
      const database = await invoke<VideoDatabase>("add_video_directory", {
        folderPath: folder,
      });

      setSelectedFolders(databaseFolders(database));
      setDatabasePath(await invoke<string>("database_file_path"));
      setVideoFiles(database.videos);
      setActorThumbnails(database.actor_thumbnails ?? {});
      setActorBios(database.actor_bios ?? {});
      setActorSocialLinks(database.actor_social_links ?? {});
      setWatchStatistics(normalizeWatchStatistics(database.watch_statistics));
      setSelectedVideo(database.videos[0] ?? null);
      setSelectedVideoPaths(
        database.videos[0] ? [database.videos[0].file_path] : [],
      );
      setRightPanelMode("video");
      setSelectedRating(null);
      setHasDatabase(true);
      setThumbnailFailures(new Set());
      startThumbnailCaching(database.videos);
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  async function refreshDatabase() {
    setErrorMessage("");
    setIsRefreshingDatabase(true);

    try {
      const report = await invoke<RefreshReport>("refresh_video_database");
      const database = report.database;

      setSelectedFolders(databaseFolders(database));
      setVideoFiles(database.videos);
      setActorThumbnails(database.actor_thumbnails ?? {});
      setActorBios(database.actor_bios ?? {});
      setActorSocialLinks(database.actor_social_links ?? {});
      setWatchStatistics(normalizeWatchStatistics(database.watch_statistics));
      setSelectedVideo((currentVideo) => {
        if (!currentVideo) {
          return database.videos[0] ?? null;
        }

        return (
          database.videos.find(
            (video) => video.file_path === currentVideo.file_path,
          ) ??
          database.videos[0] ??
          null
        );
      });
      setSelectedVideoPaths((currentPaths) => {
        const availablePaths = new Set(
          database.videos.map((video) => video.file_path),
        );
        const nextPaths = currentPaths.filter((path) => availablePaths.has(path));

        if (nextPaths.length > 0) {
          return nextPaths;
        }

        return database.videos[0] ? [database.videos[0].file_path] : [];
      });
      setRefreshReport(report);
      setThumbnailFailures(new Set());
    } catch (error) {
      setErrorMessage(String(error));
    } finally {
      setIsRefreshingDatabase(false);
    }
  }

  async function previewOrganizeVideos(videos: VideoFile[]) {
    if (videos.length === 0) {
      return;
    }

    const folder = await open({
      directory: true,
      multiple: false,
    });

    if (typeof folder !== "string") {
      return;
    }

    setErrorMessage("");
    setOrganizeResult(null);

    try {
      const preview = await invoke<OrganizePreview>("preview_organize_videos", {
        filePaths: videos.map((video) => video.file_path),
        baseFolder: folder,
        pattern: ORGANIZE_PATTERN,
      });

      setOrganizePreview(preview);
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  async function confirmOrganizeVideos() {
    if (!organizePreview) {
      return;
    }

    setErrorMessage("");
    setIsOrganizing(true);
    setIsOrganizeProgressMinimized(false);
    setOrganizeProgress({
      total: organizePreview.items.length,
      completed: 0,
      current_file: "Preparing files",
      status: "copying",
    });

    const organizeRequest = {
      filePaths: organizePreview.items.map((item) => item.file_path),
      baseFolder: organizePreview.base_folder,
      pattern: organizePreview.pattern,
    };
    setOrganizePreview(null);

    try {
      const result = await invoke<OrganizeResult>(
        "confirm_organize_videos",
        organizeRequest,
      );
      const database = result.database;
      const pathMap = new Map(
        result.items
          .filter((item) => !item.error)
          .map((item) => [item.file_path, item.target_path]),
      );

      setSelectedFolders(databaseFolders(database));
      setVideoFiles(database.videos);
      setActorThumbnails(database.actor_thumbnails ?? {});
      setActorBios(database.actor_bios ?? {});
      setActorSocialLinks(database.actor_social_links ?? {});
      setWatchStatistics(normalizeWatchStatistics(database.watch_statistics));
      setSelectedVideo((currentVideo) => {
        if (!currentVideo) {
          return database.videos[0] ?? null;
        }

        const nextPath = pathMap.get(currentVideo.file_path) ?? currentVideo.file_path;
        return (
          database.videos.find((video) => video.file_path === nextPath) ??
          database.videos[0] ??
          null
        );
      });
      setSelectedVideoPaths((currentPaths) => {
        const availablePaths = new Set(
          database.videos.map((video) => video.file_path),
        );
        const nextPaths = currentPaths
          .map((path) => pathMap.get(path) ?? path)
          .filter((path) => availablePaths.has(path));

        return nextPaths.length > 0
          ? Array.from(new Set(nextPaths))
          : database.videos[0]
            ? [database.videos[0].file_path]
            : [];
      });
      setOrganizeResult(result);
    } catch (error) {
      setErrorMessage(String(error));
    } finally {
      setIsOrganizing(false);
      setOrganizeProgress(null);
    }
  }

  async function backupSelectedVideos(videos: VideoFile[]) {
    if (videos.length === 0) {
      return;
    }

    const folder = await open({
      directory: true,
      multiple: false,
    });

    if (typeof folder !== "string") {
      return;
    }

    setErrorMessage("");
    setBackupResult(null);
    setIsBackingUp(true);
    setBackupProgress({
      total: videos.length,
      completed: 0,
      current_file: "Preparing backup",
      status: "copying",
    });

    try {
      const result = await invoke<BackupResult>("backup_videos", {
        filePaths: videos.map((video) => video.file_path),
        destinationFolder: folder,
      });

      applyDatabaseState(result.database);
      setBackupResult(result);
    } catch (error) {
      setErrorMessage(String(error));
    } finally {
      setIsBackingUp(false);
      setBackupProgress(null);
    }
  }

  async function previewRestoreBackup() {
    const file = await open({
      multiple: false,
      filters: [
        {
          name: "Backup zip",
          extensions: ["zip"],
        },
      ],
    });

    if (typeof file !== "string") {
      return;
    }

    setErrorMessage("");
    setRestoreResult(null);

    try {
      const preview = await invoke<RestorePreview>("preview_restore_backup", {
        zipPath: file,
      });
      setRestorePreview(preview);
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  async function confirmRestoreBackup() {
    if (!restorePreview) {
      return;
    }

    setErrorMessage("");
    setIsRestoring(true);
    setRestoreProgress({
      total: restorePreview.items.length,
      completed: 0,
      current_file: "Preparing restore",
      status: "copying",
    });

    const zipPath = restorePreview.zip_path;
    setRestorePreview(null);

    try {
      const result = await invoke<RestoreResult>("restore_backup", { zipPath });
      setRestoreResult(result);
    } catch (error) {
      setErrorMessage(String(error));
    } finally {
      setIsRestoring(false);
      setRestoreProgress(null);
    }
  }

  async function checkForAppUpdates() {
    setErrorMessage("");
    setUpdateStatus("");
    setUpdateDownloadProgress("");
    setPendingUpdate(null);
    setIsCheckingForUpdate(true);

    try {
      const update = await check();

      if (!update) {
        setUpdateStatus("App is up to date.");
        return;
      }

      setPendingUpdate(update);
      setUpdateStatus(
        `Update ${update.version} is available. Current version: ${update.currentVersion}.`,
      );
    } catch (error) {
      setUpdateStatus(`Could not check for updates: ${String(error)}`);
    } finally {
      setIsCheckingForUpdate(false);
    }
  }

  async function installPendingUpdate() {
    if (!pendingUpdate) {
      return;
    }

    setErrorMessage("");
    setUpdateDownloadProgress("");
    setIsInstallingUpdate(true);

    try {
      let downloadedBytes = 0;
      let contentLength = 0;

      await pendingUpdate.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          downloadedBytes = 0;
          contentLength = event.data.contentLength ?? 0;
          setUpdateDownloadProgress("Download started.");
          return;
        }

        if (event.event === "Progress") {
          downloadedBytes += event.data.chunkLength;
          setUpdateDownloadProgress(
            contentLength > 0
              ? `${formatFileSize(downloadedBytes)} / ${formatFileSize(contentLength)}`
              : `${formatFileSize(downloadedBytes)} downloaded`,
          );
          return;
        }

        setUpdateDownloadProgress("Download finished.");
      });

      setPendingUpdate(null);
      setUpdateStatus("Update installed. Restart the app to finish.");
    } catch (error) {
      setUpdateStatus(`Could not install update: ${String(error)}`);
    } finally {
      setIsInstallingUpdate(false);
    }
  }

  async function openVideo(filePath: string) {
    setErrorMessage("");

    try {
      const watchedVideo = videoFiles.find((video) => video.file_path === filePath);
      const playCount = await invoke<number>("open_video_file", { filePath });

      setVideoFiles((currentVideos) =>
        currentVideos.map((video) =>
          video.file_path === filePath
            ? { ...video, play_count: playCount }
            : video,
        ),
      );

      setSelectedVideo((currentVideo) =>
        currentVideo?.file_path === filePath
          ? { ...currentVideo, play_count: playCount }
          : currentVideo,
      );
      if (watchedVideo) {
        setWatchStatistics((currentStatistics) =>
          incrementLocalWatchStatistics(currentStatistics, watchedVideo),
        );
      }
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  async function openVideoIncognito(filePath: string) {
    setErrorMessage("");

    try {
      await invoke("open_video_file_incognito", { filePath });
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  async function openVideoDirectory(filePath: string) {
    setErrorMessage("");

    try {
      await invoke("open_video_directory", { filePath });
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  async function updateFileActorTags(videos: VideoFile[]) {
    if (videos.length === 0) {
      return;
    }

    const confirmed = window.confirm(
      `This will write title, actor, genre, date, rating, and notes from our JSON database into the selected video file metadata.\n\nActor is written as Contributing artists / Artist. Notes are written as Comments. Safe formats only: MP4, M4V, MOV.\n\nFiles selected: ${videos.length}\n\nContinue?`,
    );

    if (!confirmed) {
      return;
    }

    setErrorMessage("");

    try {
      const result = await invoke<FileTagUpdateResult>(
        "update_video_file_actor_tags",
        {
          filePaths: videos.map((video) => video.file_path),
        },
      );
      const errorSummary =
        result.errors.length > 0 ? ` ${result.errors.slice(0, 3).join(" ")}` : "";

      setErrorMessage(
        `Updated file metadata for ${result.updated_count} file(s). Skipped ${result.skipped_count}.${errorSummary}`,
      );
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  function requestDeleteSelectedVideoFiles(videos: VideoFile[]) {
    if (videos.length === 0) {
      return;
    }

    setPendingDeleteVideos(videos);
  }

  async function confirmDeleteSelectedVideoFiles() {
    if (pendingDeleteVideos.length === 0) {
      return;
    }

    setErrorMessage("");

    try {
      const result = await invoke<DeleteVideosResult>("delete_videos", {
        filePaths: pendingDeleteVideos.map((video) => video.file_path),
      });
      setPendingDeleteVideos([]);
      applyDatabaseState(result.database);
      setSelectedVideo(result.database.videos[0] ?? null);
      setSelectedVideoPaths(
        result.database.videos[0] ? [result.database.videos[0].file_path] : [],
      );
      setIsRightPanelEditing(false);

      const errorSummary =
        result.errors.length > 0 ? ` ${result.errors.slice(0, 3).join(" ")}` : "";
      setErrorMessage(
        `Moved ${result.recycled_count} file(s) to Recycle Bin. Removed ${result.removed_from_database_count} database row(s).${errorSummary}`,
      );
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  function selectedVideosForEdit() {
    if (selectedVideoPaths.length > 0) {
      const selectedPathSet = new Set(selectedVideoPaths);
      return videoFiles.filter((video) => selectedPathSet.has(video.file_path));
    }

    return selectedVideo ? [selectedVideo] : [];
  }

  function sharedStringValue(
    videos: VideoFile[],
    field:
      | "title"
      | "actor"
      | "genre"
      | "date"
      | "backup_date"
      | "backup_location"
      | "notes",
  ) {
    const firstValue = videos[0]?.[field] ?? "";

    return videos.every((video) => video[field] === firstValue)
      ? firstValue
      : "various";
  }

  function sharedNumberValue(
    videos: VideoFile[],
    field: "rating" | "play_count",
  ) {
    const firstValue = videos[0]?.[field] ?? 0;

    return videos.every((video) => (video[field] ?? 0) === firstValue)
      ? String(firstValue)
      : "various";
  }

  function sharedBooleanValue(
    videos: VideoFile[],
    field: "explicit_content",
  ): VideoEditForm["explicit_content"] {
    const firstValue = videos[0]?.[field] ?? false;

    return videos.every((video) => (video[field] ?? false) === firstValue)
      ? firstValue
        ? "true"
        : "false"
      : "various";
  }

  function valueForBatchSave(
    value: string,
    numeric = false,
    max?: number,
  ) {
    if (value === "various") {
      return null;
    }

    if (!numeric) {
      return value;
    }

    const parsedValue = Number.parseInt(value, 10) || 0;
    return max === undefined ? Math.max(0, parsedValue) : clampRating(parsedValue);
  }

  function booleanValueForBatchSave(value: VideoEditForm["explicit_content"]) {
    return value === "various" ? null : value === "true";
  }

  async function hashPassword(password: string) {
    const bytes = new TextEncoder().encode(password);
    const hashBuffer = await crypto.subtle.digest("SHA-256", bytes);

    return Array.from(new Uint8Array(hashBuffer))
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join("");
  }

  async function verifyExplicitPassword() {
    if (!explicitContentPasswordHash) {
      return true;
    }

    const password = window.prompt("Enter explicit content password");

    if (!password) {
      return false;
    }

    return (await hashPassword(password)) === explicitContentPasswordHash;
  }

  async function setExplicitPassword() {
    if (explicitContentPasswordHash && !(await verifyExplicitPassword())) {
      setErrorMessage("Incorrect password.");
      return;
    }

    const password = window.prompt("Set explicit content password");

    if (!password) {
      return;
    }

    const confirmation = window.prompt("Confirm explicit content password");

    if (password !== confirmation) {
      setErrorMessage("Passwords did not match.");
      return;
    }

    setErrorMessage("");
    setExplicitContentPasswordHash(await hashPassword(password));
  }

  async function toggleHideExplicitContent(shouldHide: boolean) {
    setErrorMessage("");

    if (shouldHide && !explicitContentPasswordHash) {
      const password = window.prompt(
        "Create a password before hiding explicit content",
      );

      if (!password) {
        return;
      }

      const confirmation = window.prompt("Confirm explicit content password");

      if (password !== confirmation) {
        setErrorMessage("Passwords did not match.");
        return;
      }

      setExplicitContentPasswordHash(await hashPassword(password));
      setHideExplicitContent(true);
      return;
    }

    if (!shouldHide && !(await verifyExplicitPassword())) {
      setErrorMessage("Incorrect password.");
      return;
    }

    setHideExplicitContent(shouldHide);
  }

  async function saveSelectedVideo() {
    const selectedVideos = selectedVideosForEdit();

    if (selectedVideos.length === 0) {
      return;
    }

    setErrorMessage("");

    try {
      if (selectedVideos.length > 1) {
        const updatedVideos = await invoke<VideoFile[]>(
          "update_multiple_video_metadata",
          {
            filePaths: selectedVideos.map((video) => video.file_path),
            title: valueForBatchSave(editForm.title),
            actor: valueForBatchSave(editForm.actor),
            genre: valueForBatchSave(editForm.genre),
            date: valueForBatchSave(editForm.date),
            backupDate: valueForBatchSave(editForm.backup_date),
            backupLocation: valueForBatchSave(editForm.backup_location),
            notes: valueForBatchSave(editForm.notes),
            explicitContent: booleanValueForBatchSave(editForm.explicit_content),
            rating: valueForBatchSave(editForm.rating, true, 10),
            playCount: valueForBatchSave(editForm.play_count, true),
          },
        );
        const updatedByPath = new Map(
          updatedVideos.map((video) => [video.file_path, video]),
        );

        setVideoFiles((currentVideos) =>
          currentVideos.map((video) => updatedByPath.get(video.file_path) ?? video),
        );
        setSelectedVideo(
          updatedVideos.find(
            (video) => video.file_path === selectedVideo?.file_path,
          ) ?? updatedVideos[0],
        );
        setIsRightPanelEditing(false);
        return;
      }

      const selectedVideoForSave = selectedVideos[0];
      const updatedVideo = await invoke<VideoFile>("update_video_metadata", {
        filePath: selectedVideoForSave.file_path,
        title: editForm.title,
        actor: editForm.actor,
        genre: editForm.genre,
        date: editForm.date,
        backupDate: editForm.backup_date,
        backupLocation: editForm.backup_location,
        notes: editForm.notes,
        explicitContent: editForm.explicit_content === "true",
        rating: clampRating(Number.parseInt(editForm.rating, 10) || 0),
        playCount: Number.parseInt(editForm.play_count, 10) || 0,
      });

      setVideoFiles((currentVideos) =>
        currentVideos.map((video) =>
          video.file_path === updatedVideo.file_path ? updatedVideo : video,
        ),
      );
      setSelectedVideo(updatedVideo);
      setIsRightPanelEditing(false);
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  async function selectActorThumbnail(actorName: string) {
    const file = await open({
      multiple: false,
      filters: [
        {
          name: "Images",
          extensions: ["png", "jpg", "jpeg", "webp", "bmp"],
        },
      ],
    });

    if (typeof file !== "string") {
      return;
    }

    setErrorMessage("");

    try {
      const imageBytes = await invoke<number[]>("read_image_file", {
        filePath: file,
      });
      const extension = file.split(".").pop()?.toLowerCase();
      const mimeType =
        extension === "png"
          ? "image/png"
          : extension === "webp"
            ? "image/webp"
            : extension === "bmp"
              ? "image/bmp"
              : "image/jpeg";
      const objectUrl = URL.createObjectURL(
        new Blob([new Uint8Array(imageBytes)], { type: mimeType }),
      );

      setActorCrop((currentCrop) => {
        if (currentCrop?.objectUrl) {
          URL.revokeObjectURL(currentCrop.objectUrl);
        }

        return {
          actorName,
          imageSrc: objectUrl,
          objectUrl,
          crop: {
            x: 25,
            y: 12.5,
            width: 50,
            height: 75,
          },
        };
      });
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  function prepareActorCrop(event: SyntheticEvent<HTMLImageElement>) {
    const image = event.currentTarget;
    const widthPercent = 50;
    let heightPercent = ((widthPercent / 100) * image.clientWidth * 1.5) /
      image.clientHeight *
      100;
    let safeWidthPercent = widthPercent;

    if (heightPercent > 90) {
      heightPercent = 90;
      safeWidthPercent = ((heightPercent / 100) * image.clientHeight) /
        1.5 /
        image.clientWidth *
        100;
    }

    setActorCrop((currentCrop) =>
      currentCrop
        ? {
            ...currentCrop,
            crop: {
              x: (100 - safeWidthPercent) / 2,
              y: (100 - heightPercent) / 2,
              width: safeWidthPercent,
              height: heightPercent,
            },
          }
        : currentCrop,
    );
  }

  function clampCrop(crop: CropRect) {
    return {
      ...crop,
      x: Math.max(0, Math.min(100 - crop.width, crop.x)),
      y: Math.max(0, Math.min(100 - crop.height, crop.y)),
    };
  }

  function updateActorCropSize(sizePercent: number) {
    const image = actorCropImageRef.current;

    if (!image) {
      return;
    }

    const width = sizePercent;
    const height = ((width / 100) * image.clientWidth * 1.5) /
      image.clientHeight *
      100;

    if (height > 95) {
      return;
    }

    setActorCrop((currentCrop) =>
      currentCrop
        ? {
            ...currentCrop,
            crop: clampCrop({
              x: currentCrop.crop.x,
              y: currentCrop.crop.y,
              width,
              height,
            }),
          }
        : currentCrop,
    );
  }

  function startActorCropDrag(event: PointerEvent<HTMLDivElement>) {
    if (!actorCrop) {
      return;
    }

    event.currentTarget.setPointerCapture(event.pointerId);
    actorCropDragRef.current = {
      startX: event.clientX,
      startY: event.clientY,
      cropX: actorCrop.crop.x,
      cropY: actorCrop.crop.y,
    };
  }

  function moveActorCrop(event: PointerEvent<HTMLDivElement>) {
    const drag = actorCropDragRef.current;
    const image = actorCropImageRef.current;

    if (!drag || !image) {
      return;
    }

    const deltaX = ((event.clientX - drag.startX) / image.clientWidth) * 100;
    const deltaY = ((event.clientY - drag.startY) / image.clientHeight) * 100;

    setActorCrop((currentCrop) =>
      currentCrop
        ? {
            ...currentCrop,
            crop: clampCrop({
              ...currentCrop.crop,
              x: drag.cropX + deltaX,
              y: drag.cropY + deltaY,
            }),
          }
        : currentCrop,
    );
  }

  function stopActorCropDrag() {
    actorCropDragRef.current = null;
  }

  async function saveCroppedActorThumbnail() {
    if (!actorCrop || !actorCropImageRef.current) {
      return;
    }

    setErrorMessage("");

    try {
      const image = actorCropImageRef.current;
      const sourceX = (actorCrop.crop.x / 100) * image.naturalWidth;
      const sourceY = (actorCrop.crop.y / 100) * image.naturalHeight;
      const sourceWidth = (actorCrop.crop.width / 100) * image.naturalWidth;
      const sourceHeight = (actorCrop.crop.height / 100) * image.naturalHeight;
      const canvas = document.createElement("canvas");
      canvas.width = 400;
      canvas.height = 600;

      const context = canvas.getContext("2d");
      if (!context) {
        throw new Error("Could not create actor thumbnail canvas.");
      }

      context.drawImage(
        image,
        sourceX,
        sourceY,
        sourceWidth,
        sourceHeight,
        0,
        0,
        canvas.width,
        canvas.height,
      );

      const blob = await new Promise<Blob>((resolve, reject) => {
        canvas.toBlob(
          (thumbnailBlob) => {
            if (thumbnailBlob) {
              resolve(thumbnailBlob);
            } else {
              reject(new Error("Could not create actor thumbnail image."));
            }
          },
          "image/jpeg",
          0.86,
        );
      });
      const bytes = new Uint8Array(await blob.arrayBuffer());
      const thumbnails = await invoke<Record<string, string>>(
        "save_actor_thumbnail",
        {
          actorName: actorCrop.actorName,
          imageBytes: Array.from(bytes),
        },
      );
      setActorThumbnails(thumbnails);
      setActorThumbnailRefreshKeys((currentKeys) => ({
        ...currentKeys,
        [actorCrop.actorName]: Date.now(),
      }));
      setActorCrop(null);
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  async function saveActorBio(actorName: string) {
    setErrorMessage("");

    try {
      const bios = await invoke<Record<string, string>>("update_actor_bio", {
        actorName,
        bio: actorBios[actorName] ?? "",
      });
      setActorBios(bios);
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  function blankActorSocialLinks(): ActorSocialLinks {
    return {
      website: "",
      imdb: "",
      instagram: "",
      x: "",
      youtube: "",
    };
  }

  function actorSocialValues(actorName: string) {
    return actorSocialLinks[actorName] ?? blankActorSocialLinks();
  }

  async function saveActorSocialLinks(actorName: string) {
    setErrorMessage("");

    try {
      const socialLinks = await invoke<Record<string, ActorSocialLinks>>(
        "update_actor_social_links",
        {
          actorName,
          socialLinks: actorSocialValues(actorName),
        },
      );
      setActorSocialLinks(socialLinks);
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  async function saveActorProfile(actorName: string) {
    await saveActorBio(actorName);
    await saveActorSocialLinks(actorName);
    setIsRightPanelEditing(false);
  }

  function updateNumberField(
    field: "rating" | "play_count",
    change: number,
    max?: number,
  ) {
    setEditForm((currentForm) => {
      const currentValue = Number.parseInt(currentForm[field], 10) || 0;
      const nextValue = Math.max(0, currentValue + change);

      return {
        ...currentForm,
        [field]: String(max === undefined ? nextValue : Math.min(max, nextValue)),
      };
    });
  }

  function currentNavigationSnapshot(): NavigationSnapshot {
    return {
      activeView: activeViewRef.current,
      selectedActor,
      selectedGenre,
      selectedRating,
      selectedVideoPath: selectedVideo?.file_path ?? "",
      selectedVideoPaths,
      rightPanelMode,
      isRightPanelEditing,
    };
  }

  function pushNavigationSnapshot() {
    navigationHistoryRef.current = [
      ...navigationHistoryRef.current,
      currentNavigationSnapshot(),
    ].slice(-30);
  }

  function restoreNavigationSnapshot(snapshot: NavigationSnapshot) {
    const validSelectedPaths = snapshot.selectedVideoPaths.filter((filePath) =>
      videoFiles.some((video) => video.file_path === filePath),
    );
    const restoredSelectedVideo =
      videoFiles.find((video) => video.file_path === snapshot.selectedVideoPath) ??
      videoFiles.find((video) => validSelectedPaths.includes(video.file_path)) ??
      null;

    activeViewRef.current = snapshot.activeView;
    setActiveView(snapshot.activeView);
    setSelectedActor(snapshot.selectedActor);
    setSelectedGenre(snapshot.selectedGenre);
    setSelectedRating(snapshot.selectedRating);
    setSelectedVideo(restoredSelectedVideo);
    setSelectedVideoPaths(
      validSelectedPaths.length > 0
        ? validSelectedPaths
        : restoredSelectedVideo
          ? [restoredSelectedVideo.file_path]
          : [],
    );
    setRightPanelMode(snapshot.rightPanelMode);
    setIsRightPanelEditing(snapshot.isRightPanelEditing);
  }

  function navigateToView(view: ViewMode) {
    const currentView = activeViewRef.current;

    if (currentView === view) {
      return;
    }

    pushNavigationSnapshot();
    activeViewRef.current = view;
    setActiveView(view);
  }

  function goBackView() {
    const previousState = navigationHistoryRef.current.pop();

    if (!previousState) {
      return;
    }

    restoreNavigationSnapshot(previousState);
  }

  function showActorVideos(video: VideoFile) {
    const actorName = actorNames(video)[0];

    if (activeViewRef.current === "actors") {
      pushNavigationSnapshot();
    }

    setSelectedVideo(video);
    setSelectedVideoPaths([video.file_path]);
    setIsRightPanelEditing(false);
    navigateToView("actors");
    setSelectedActor(actorName);
    setSelectedGenre("");
    setSelectedRating(null);
    setActorFilter("all");
    setGenreFilters([]);
    setRatingFilters([]);
    setRightPanelMode("actor");
  }

  function showActorVideosByName(actorName: string, videos: VideoFile[]) {
    const firstVideo = videos[0];

    if (activeViewRef.current === "actors") {
      pushNavigationSnapshot();
    }

    if (firstVideo) {
      setSelectedVideo(firstVideo);
      setSelectedVideoPaths([firstVideo.file_path]);
    }

    setIsRightPanelEditing(false);
    navigateToView("actors");
    setSelectedActor(actorName);
    setSelectedGenre("");
    setSelectedRating(null);
    setActorFilter("all");
    setGenreFilters([]);
    setRatingFilters([]);
    setRightPanelMode("actor");
  }

  function artworkSource(video: VideoFile) {
    const source = hasCachedThumbnail(video) ? video.artwork_thumbnail : "";
    const refreshKey = thumbnailRefreshKeys[video.file_path];
    const url = convertFileSrc(source);

    return refreshKey ? `${url}?refresh=${refreshKey}` : url;
  }

  function hasCachedThumbnail(video: VideoFile) {
    const thumbnail = video.artwork_thumbnail.toLowerCase();

    return (
      video.artwork_thumbnail !== "" &&
      video.artwork_thumbnail !== "embedded" &&
      video.artwork_thumbnail !== video.file_path &&
      (thumbnail.endsWith(".jpg") ||
        thumbnail.endsWith(".jpeg") ||
        thumbnail.endsWith(".png"))
    );
  }

  function hasThumbnailFailed(video: VideoFile) {
    return thumbnailFailures.has(video.file_path);
  }

  async function startThumbnailCaching(videos: VideoFile[]) {
    if (isGeneratingThumbnails.current) {
      return;
    }

    isGeneratingThumbnails.current = true;

    for (const video of videos) {
      if (hasCachedThumbnail(video)) {
        if (!video.resolution || !video.bitrate) {
          await updateTechnicalMetadataForVideo(video);
        }
        continue;
      }

      try {
        const thumbnail = await createVideoThumbnail(video);
        setThumbnailFailures((currentFailures) => {
          const nextFailures = new Set(currentFailures);
          nextFailures.delete(video.file_path);
          return nextFailures;
        });

        setVideoFiles((currentVideos) =>
          currentVideos.map((currentVideo) =>
            currentVideo.file_path === video.file_path
              ? {
                  ...currentVideo,
                  artwork_thumbnail: thumbnail.thumbnailPath,
                  resolution: thumbnail.resolution,
                  bitrate: thumbnail.bitrate,
                }
              : currentVideo,
          ),
        );

        setSelectedVideo((currentVideo) =>
          currentVideo?.file_path === video.file_path
            ? {
                ...currentVideo,
                artwork_thumbnail: thumbnail.thumbnailPath,
                resolution: thumbnail.resolution,
                bitrate: thumbnail.bitrate,
              }
            : currentVideo,
        );
      } catch {
        setThumbnailFailures((currentFailures) => {
          const nextFailures = new Set(currentFailures);
          nextFailures.add(video.file_path);
          return nextFailures;
        });
      }
    }

    isGeneratingThumbnails.current = false;
  }

  async function createThumbnailForVideo(video: VideoFile) {
    setErrorMessage("");

    try {
      const thumbnail = await createVideoThumbnail(video);
      setThumbnailRefreshKeys((currentKeys) => ({
        ...currentKeys,
        [video.file_path]: Date.now(),
      }));
      setThumbnailFailures((currentFailures) => {
        const nextFailures = new Set(currentFailures);
        nextFailures.delete(video.file_path);
        return nextFailures;
      });

      setVideoFiles((currentVideos) =>
        currentVideos.map((currentVideo) =>
          currentVideo.file_path === video.file_path
            ? {
                ...currentVideo,
                artwork_thumbnail: thumbnail.thumbnailPath,
                resolution: thumbnail.resolution,
                bitrate: thumbnail.bitrate,
              }
            : currentVideo,
        ),
      );

      setSelectedVideo((currentVideo) =>
        currentVideo?.file_path === video.file_path
          ? {
              ...currentVideo,
              artwork_thumbnail: thumbnail.thumbnailPath,
              resolution: thumbnail.resolution,
              bitrate: thumbnail.bitrate,
            }
          : currentVideo,
      );
    } catch (error) {
      setThumbnailFailures((currentFailures) => {
        const nextFailures = new Set(currentFailures);
        nextFailures.add(video.file_path);
        return nextFailures;
      });
      setErrorMessage(String(error));
    }
  }

  async function createThumbnailsForVideos(videos: VideoFile[]) {
    setErrorMessage("");

    for (const video of videos) {
      await createThumbnailForVideo(video);
    }
  }

  async function createVideoThumbnail(
    video: VideoFile,
  ): Promise<VideoThumbnailResult> {
    const videoElement = document.createElement("video");
    videoElement.crossOrigin = "anonymous";
    videoElement.muted = true;
    videoElement.preload = "metadata";

    const metadataReady = waitForVideoEvent(videoElement, "loadedmetadata");
    videoElement.src = convertFileSrc(video.file_path);
    videoElement.load();
    await metadataReady;
    const technicalMetadata = videoTechnicalMetadata(videoElement, video);

    const requestedSeekTime =
      Number.parseFloat(thumbnailFrameSecond) >= 0
        ? Number.parseFloat(thumbnailFrameSecond)
        : 1;
    const seekTime =
      Number.isFinite(videoElement.duration) && videoElement.duration > 0
        ? Math.min(requestedSeekTime, Math.max(0, videoElement.duration - 0.1))
        : 0;

    if (seekTime > 0) {
      const seekReady = waitForVideoEvent(videoElement, "seeked");
      videoElement.currentTime = seekTime;
      await seekReady;
    } else {
      await waitForVideoEvent(videoElement, "loadeddata");
    }

    const canvas = document.createElement("canvas");
    canvas.width = 320;
    canvas.height = 180;

    const context = canvas.getContext("2d");
    if (!context) {
      throw new Error("Could not create thumbnail canvas.");
    }

    drawVideoFrameCover(videoElement, context, canvas.width, canvas.height);

    const blob = await new Promise<Blob>((resolve, reject) => {
      canvas.toBlob(
        (thumbnailBlob) => {
          if (thumbnailBlob) {
            resolve(thumbnailBlob);
          } else {
            reject(new Error("Could not create thumbnail image."));
          }
        },
        "image/jpeg",
        0.78,
      );
    });

    const bytes = new Uint8Array(await blob.arrayBuffer());

    const thumbnailPath = await invoke<string>("save_video_thumbnail", {
      filePath: video.file_path,
      imageBytes: Array.from(bytes),
    });
    await saveVideoTechnicalMetadata(video.file_path, technicalMetadata);

    return {
      thumbnailPath,
      ...technicalMetadata,
    };
  }

  function drawVideoFrameCover(
    videoElement: HTMLVideoElement,
    context: CanvasRenderingContext2D,
    targetWidth: number,
    targetHeight: number,
  ) {
    const sourceWidth = videoElement.videoWidth || targetWidth;
    const sourceHeight = videoElement.videoHeight || targetHeight;
    const sourceRatio = sourceWidth / sourceHeight;
    const targetRatio = targetWidth / targetHeight;
    let cropWidth = sourceWidth;
    let cropHeight = sourceHeight;
    let cropX = 0;
    let cropY = 0;

    if (sourceRatio > targetRatio) {
      cropWidth = sourceHeight * targetRatio;
      cropX = (sourceWidth - cropWidth) / 2;
    } else if (sourceRatio < targetRatio) {
      cropHeight = sourceWidth / targetRatio;
      cropY = (sourceHeight - cropHeight) / 2;
    }

    context.drawImage(
      videoElement,
      cropX,
      cropY,
      cropWidth,
      cropHeight,
      0,
      0,
      targetWidth,
      targetHeight,
    );
  }

  async function updateTechnicalMetadataForVideo(video: VideoFile) {
    try {
      const technicalMetadata = await loadVideoTechnicalMetadata(video);
      await saveVideoTechnicalMetadata(video.file_path, technicalMetadata);
      setVideoFiles((currentVideos) =>
        currentVideos.map((currentVideo) =>
          currentVideo.file_path === video.file_path
            ? { ...currentVideo, ...technicalMetadata }
            : currentVideo,
        ),
      );
      setSelectedVideo((currentVideo) =>
        currentVideo?.file_path === video.file_path
          ? { ...currentVideo, ...technicalMetadata }
          : currentVideo,
      );
    } catch {
      // Some formats can still show thumbnails from cache but fail browser metadata.
    }
  }

  async function loadVideoTechnicalMetadata(
    video: VideoFile,
  ): Promise<VideoTechnicalMetadata> {
    const videoElement = document.createElement("video");
    videoElement.crossOrigin = "anonymous";
    videoElement.muted = true;
    videoElement.preload = "metadata";

    const metadataReady = waitForVideoEvent(videoElement, "loadedmetadata");
    videoElement.src = convertFileSrc(video.file_path);
    videoElement.load();
    await metadataReady;

    return videoTechnicalMetadata(videoElement, video);
  }

  function videoTechnicalMetadata(
    videoElement: HTMLVideoElement,
    video: VideoFile,
  ): VideoTechnicalMetadata {
    const resolution =
      videoElement.videoWidth > 0 && videoElement.videoHeight > 0
        ? `${videoElement.videoWidth}x${videoElement.videoHeight}`
        : "";
    const bitrate =
      Number.isFinite(videoElement.duration) && videoElement.duration > 0
        ? formatBitrate((video.filesize * 8) / videoElement.duration)
        : "";

    return { resolution, bitrate };
  }

  function formatBitrate(bitsPerSecond: number) {
    if (!Number.isFinite(bitsPerSecond) || bitsPerSecond <= 0) {
      return "";
    }

    if (bitsPerSecond >= 1_000_000) {
      return `${(bitsPerSecond / 1_000_000).toFixed(1)} Mbps`;
    }

    return `${Math.round(bitsPerSecond / 1_000)} kbps`;
  }

  function formatAddedAt(value: string) {
    if (!value) {
      return "";
    }

    const numericTimestamp = Number(value);
    const date = Number.isFinite(numericTimestamp)
      ? new Date(numericTimestamp * 1000)
      : new Date(value);

    return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString();
  }

  function textOrDash(value: string | number | boolean) {
    if (typeof value === "boolean") {
      return value ? "Yes" : "No";
    }

    return value === "" || value === 0 ? "-" : String(value);
  }

  async function saveVideoTechnicalMetadata(
    filePath: string,
    metadata: VideoTechnicalMetadata,
  ) {
    if (!metadata.resolution && !metadata.bitrate) {
      return;
    }

    await invoke<VideoFile>("update_video_technical_metadata", {
      filePath,
      resolution: metadata.resolution,
      bitrate: metadata.bitrate,
    });
  }

  function waitForVideoEvent(
    videoElement: HTMLVideoElement,
    eventName: "loadedmetadata" | "loadeddata" | "seeked",
  ) {
    return new Promise<void>((resolve, reject) => {
      if (
        (eventName === "loadedmetadata" && videoElement.readyState >= 1) ||
        (eventName === "loadeddata" && videoElement.readyState >= 2)
      ) {
        resolve();
        return;
      }

      let timeout = 0;

      const cleanup = () => {
        window.clearTimeout(timeout);
        videoElement.removeEventListener(eventName, onEvent);
        videoElement.removeEventListener("error", onError);
      };

      const onEvent = () => {
        cleanup();
        resolve();
      };

      const onError = () => {
        cleanup();
        reject(new Error("Could not load video for thumbnail."));
      };

      videoElement.addEventListener(eventName, onEvent, { once: true });
      videoElement.addEventListener("error", onError, { once: true });

      timeout = window.setTimeout(() => {
        cleanup();
        reject(new Error(`Timed out while waiting for ${eventName}.`));
      }, 8000);
    });
  }

  function formatFileSize(bytes: number) {
    if (!bytes) {
      return "";
    }

    const megabytes = bytes / 1024 / 1024;
    return `${megabytes.toFixed(1)} MB`;
  }

  function totalVideoSize() {
    return videoFiles.reduce((total, video) => total + (video.filesize ?? 0), 0);
  }

  function updateThumbnailSize(value: string) {
    const parsedValue = Number.parseInt(value, 10);
    const safeValue = Number.isFinite(parsedValue)
      ? Math.max(120, Math.min(360, parsedValue))
      : 180;

    setGridSize(String(safeValue));
  }

  function appFontSizeVars(): FontSizeVars {
    const sizes = {
      small: {
        "--app-button-font-size": "0.9rem",
        "--thumbnail-label-font-size": "0.98rem",
        "--thumbnail-label-small-font-size": "0.76rem",
        "--right-panel-font-size": "1.1rem",
        "--right-thumbnail-label-font-size": "1.25rem",
      },
      medium: {
        "--app-button-font-size": "1rem",
        "--thumbnail-label-font-size": "1.12rem",
        "--thumbnail-label-small-font-size": "0.87rem",
        "--right-panel-font-size": "1.35rem",
        "--right-thumbnail-label-font-size": "1.43rem",
      },
      large: {
        "--app-button-font-size": "1.15rem",
        "--thumbnail-label-font-size": "1.27rem",
        "--thumbnail-label-small-font-size": "0.99rem",
        "--right-panel-font-size": "1.69rem",
        "--right-thumbnail-label-font-size": "1.63rem",
      },
    } satisfies Record<AppFontSize, FontSizeVars>;

    return sizes[appFontSize];
  }

  function databaseFolders(database: VideoDatabase) {
    const paths = database.folder_paths?.length
      ? database.folder_paths
      : database.folder_path
        ? [database.folder_path]
        : [];

    return Array.from(new Set(paths.filter(Boolean)));
  }

  function applyDatabaseState(database: VideoDatabase) {
    setSelectedFolders(databaseFolders(database));
    setVideoFiles(database.videos);
    setActorThumbnails(database.actor_thumbnails ?? {});
    setActorBios(database.actor_bios ?? {});
    setActorSocialLinks(database.actor_social_links ?? {});
    setWatchStatistics(normalizeWatchStatistics(database.watch_statistics));
  }

  function emptyWatchStatistics(): WatchStatistics {
    return {
      total_watches: 0,
      actor_counts: {},
      genre_counts: {},
      year_counts: {},
    };
  }

  function normalizeWatchStatistics(statistics?: WatchStatistics) {
    return {
      total_watches: statistics?.total_watches ?? 0,
      actor_counts: statistics?.actor_counts ?? {},
      genre_counts: statistics?.genre_counts ?? {},
      year_counts: statistics?.year_counts ?? {},
    };
  }

  function clampRating(rating: number) {
    return Math.max(0, Math.min(10, rating));
  }

  function formatRatingLabel(rating: number) {
    if (rating === 0) {
      return "Unrated";
    }

    const starValue = rating / 2;
    return `${Number.isInteger(starValue) ? starValue.toFixed(0) : starValue} stars`;
  }

  function ratingValue(video: VideoFile) {
    return clampRating(video.rating ?? 0);
  }

  function ratingGroupName(video: VideoFile) {
    return [formatRatingLabel(ratingValue(video))];
  }

  function renderRatingStars(
    rating: number,
    onChange?: (rating: number) => void,
  ) {
    const safeRating = clampRating(rating);

    return (
      <div
        className={onChange ? "star-rating editable" : "star-rating"}
        aria-label={formatRatingLabel(safeRating)}
      >
        {Array.from({ length: 5 }, (_, index) => {
          const starNumber = index + 1;
          const fillPercent = Math.max(
            0,
            Math.min(100, (safeRating - index * 2) * 50),
          );
          const star = (
            <>
              <span className="star-empty">★</span>
              <span className="star-fill" style={{ width: `${fillPercent}%` }}>
                ★
              </span>
            </>
          );

          if (!onChange) {
            return (
              <span className="star-button" key={starNumber}>
                {star}
              </span>
            );
          }

          return (
            <button
              aria-label={`${starNumber} star`}
              className="star-button"
              key={starNumber}
              type="button"
              onClick={(event) => {
                const bounds = event.currentTarget.getBoundingClientRect();
                const isLeftHalf = event.clientX - bounds.left < bounds.width / 2;
                onChange(index * 2 + (isLeftHalf ? 1 : 2));
              }}
            >
              {star}
            </button>
          );
        })}
      </div>
    );
  }

  function actorNames(video: VideoFile) {
    return splitTagList(video.actor, "Unknown actor");
  }

  function genreNames(video: VideoFile) {
    return splitTagList(video.genre, "Unknown genre");
  }

  function dateYearName(video: VideoFile) {
    const match = video.date.match(/\d{4}/);
    return match ? match[0] : "Unknown Year";
  }

  function incrementLocalWatchStatistics(
    statistics: WatchStatistics,
    video: VideoFile,
  ): WatchStatistics {
    let nextStatistics = {
      total_watches: statistics.total_watches + 1,
      actor_counts: { ...statistics.actor_counts },
      genre_counts: { ...statistics.genre_counts },
      year_counts: { ...statistics.year_counts },
    };

    for (const actor of actorNames(video)) {
      nextStatistics = incrementLocalStat(nextStatistics, "actor_counts", actor);
    }

    for (const genre of genreNames(video)) {
      nextStatistics = incrementLocalStat(nextStatistics, "genre_counts", genre);
    }

    return incrementLocalStat(nextStatistics, "year_counts", dateYearName(video));
  }

  function incrementLocalStat(
    statistics: WatchStatistics,
    field: "actor_counts" | "genre_counts" | "year_counts",
    key: string,
  ) {
    return {
      ...statistics,
      [field]: {
        ...statistics[field],
        [key]: (statistics[field][key] ?? 0) + 1,
      },
    };
  }

  function uniqueTagOptions(nameGetter: (video: VideoFile) => string[]) {
    return Array.from(
      new Set(contentVisibleVideos(videoFiles).flatMap((video) => nameGetter(video))),
    ).sort((first, second) => first.localeCompare(second));
  }

  function actorFilterOptions() {
    return uniqueTagOptions(actorNames);
  }

  function genreFilterOptions() {
    return uniqueTagOptions(genreNames);
  }

  function autocompleteOptions(field: AutocompleteField) {
    return field === "actor" ? actorFilterOptions() : genreFilterOptions();
  }

  function autocompleteSearchPart(value: string) {
    const separatorIndex = Math.max(
      value.lastIndexOf(","),
      value.lastIndexOf(";"),
      value.lastIndexOf("|"),
      value.lastIndexOf("/"),
    );

    return {
      prefix: separatorIndex >= 0 ? value.slice(0, separatorIndex + 1) : "",
      search: value.slice(separatorIndex + 1).trimStart(),
      leadingSpace:
        separatorIndex >= 0 && value.slice(separatorIndex + 1).startsWith(" ")
          ? " "
          : "",
    };
  }

  function filteredAutocompleteOptions(field: AutocompleteField, value: string) {
    const { search } = autocompleteSearchPart(value);
    const normalizedSearch = search.toLowerCase();
    const usedValues = new Set(
      value
        .split(/[,;|/]+/)
        .map((part) => part.trim().toLowerCase())
        .filter(Boolean),
    );

    return autocompleteOptions(field)
      .filter((option) => {
        const normalizedOption = option.toLowerCase();
        return (
          !usedValues.has(normalizedOption) &&
          (normalizedSearch === "" || normalizedOption.includes(normalizedSearch))
        );
      })
      .slice(0, 8);
  }

  function applyAutocompleteSuggestion(
    field: AutocompleteField,
    suggestion: string,
  ) {
    setEditForm((currentForm) => {
      const { prefix, leadingSpace } = autocompleteSearchPart(currentForm[field]);
      const separatorSpace = prefix && !leadingSpace ? " " : leadingSpace;

      return {
        ...currentForm,
        [field]: `${prefix}${separatorSpace}${suggestion}`,
      };
    });
    setAutocompleteState(null);
  }

  function handleAutocompleteKeyDown(
    event: ReactKeyboardEvent<HTMLInputElement>,
    field: AutocompleteField,
    value: string,
  ) {
    const options = filteredAutocompleteOptions(field, value);

    if (event.key === "Escape") {
      setAutocompleteState(null);
      return;
    }

    if (options.length === 0) {
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      setAutocompleteState((currentState) => ({
        field,
        activeIndex:
          currentState?.field === field
            ? (currentState.activeIndex + 1) % options.length
            : 0,
      }));
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      setAutocompleteState((currentState) => ({
        field,
        activeIndex:
          currentState?.field === field
            ? (currentState.activeIndex - 1 + options.length) % options.length
            : options.length - 1,
      }));
      return;
    }

    if (
      (event.key === "Enter" || event.key === "Tab") &&
      autocompleteState?.field === field
    ) {
      const suggestion = options[autocompleteState.activeIndex] ?? options[0];

      if (suggestion) {
        event.preventDefault();
        applyAutocompleteSuggestion(field, suggestion);
      }
    }
  }

  function ratingFilterOptions() {
    return [0, ...Array.from({ length: 10 }, (_, index) => index + 1)];
  }

  function toggleGenreFilter(genre: string) {
    setGenreFilters((currentGenres) =>
      currentGenres.includes(genre)
        ? currentGenres.filter((currentGenre) => currentGenre !== genre)
        : [...currentGenres, genre],
    );
  }

  function toggleRatingFilter(rating: number) {
    setRatingFilters((currentRatings) =>
      currentRatings.includes(rating)
        ? currentRatings.filter((currentRating) => currentRating !== rating)
        : [...currentRatings, rating],
    );
  }

  function genreFilterLabel() {
    if (genreFilters.length === 0) {
      return "All genres";
    }

    if (genreFilters.length === 1) {
      return genreFilters[0];
    }

    return `${genreFilters.length} genres`;
  }

  function ratingFilterLabel() {
    if (ratingFilters.length === 0) {
      return "All ratings";
    }

    if (ratingFilters.length === 1) {
      return formatRatingLabel(ratingFilters[0]);
    }

    return `${ratingFilters.length} ratings`;
  }

  function splitTagList(value: string, fallback: string) {
    const parts = value
      .split(/[,;|/]+/)
      .map((part) => part.trim())
      .filter(Boolean);

    return parts.length > 0 ? parts : [fallback];
  }

  function videoDisplayName(video: VideoFile) {
    return video.title || video.filename;
  }

  function clearHoverPreviewTimers() {
    if (hoverPreviewDelayRef.current !== null) {
      window.clearTimeout(hoverPreviewDelayRef.current);
      hoverPreviewDelayRef.current = null;
    }

    if (hoverThumbnailDelayRef.current !== null) {
      window.clearTimeout(hoverThumbnailDelayRef.current);
      hoverThumbnailDelayRef.current = null;
    }
  }

  function startHoverPreview(event: MouseEvent<HTMLElement>, video: VideoFile) {
    clearHoverPreviewTimers();
    hoverPreviewVideoRef.current = video;
    hoverPreviewPositionRef.current = {
      x: event.clientX,
      y: event.clientY,
    };
    hoverPreviewDelayRef.current = window.setTimeout(() => {
      setHoverPreview({
        video,
        x: hoverPreviewPositionRef.current.x,
        y: hoverPreviewPositionRef.current.y,
        showThumbnail: false,
      });
    }, 500);
    hoverThumbnailDelayRef.current = window.setTimeout(() => {
      setHoverPreview((currentPreview) =>
        currentPreview?.video.file_path === video.file_path
          ? { ...currentPreview, showThumbnail: true }
          : currentPreview,
      );
    }, 1000);
  }

  function moveHoverPreview(event: MouseEvent<HTMLElement>) {
    hoverPreviewPositionRef.current = {
      x: event.clientX,
      y: event.clientY,
    };
    setHoverPreview((currentPreview) =>
      currentPreview
        ? {
            ...currentPreview,
            x: event.clientX,
            y: event.clientY,
          }
        : currentPreview,
    );
  }

  function stopHoverPreview() {
    clearHoverPreviewTimers();
    hoverPreviewVideoRef.current = null;
    setHoverPreview(null);
  }

  function isVideoVisibleByExplicitSetting(video: VideoFile) {
    return !hideExplicitContent || !video.explicit_content;
  }

  function matchesFilter(video: VideoFile) {
    if (!isVideoVisibleByExplicitSetting(video)) {
      return false;
    }

    const searchText = filterText.trim().toLowerCase();

    if (!searchText) {
      return true;
    }

    return [
      video.filename,
      video.title,
      video.actor,
      video.genre,
      video.date,
      formatRatingLabel(ratingValue(video)),
      String(ratingValue(video)),
      String(video.play_count ?? 0),
    ]
      .join(" ")
      .toLowerCase()
      .includes(searchText);
  }

  function sortVideos(videos: VideoFile[]) {
    return [...videos].sort((first, second) => {
      const primaryDifference = compareVideos(first, second, sortMode);

      if (primaryDifference !== 0 || secondarySortMode === "none") {
        return primaryDifference;
      }

      const secondaryDifference = compareVideos(
        first,
        second,
        secondarySortMode,
      );

      if (secondaryDifference !== 0) {
        return secondaryDifference;
      }

      return compareVideos(first, second, "name");
    });
  }

  function compareVideos(first: VideoFile, second: VideoFile, mode: SortMode) {
    if (mode === "added") {
      return addedTimeValue(second) - addedTimeValue(first);
    }

    if (mode === "rating") {
      return ratingValue(second) - ratingValue(first);
    }

    if (mode === "played-count") {
      return (second.play_count ?? 0) - (first.play_count ?? 0);
    }

    if (mode === "file-size") {
      return (second.filesize ?? 0) - (first.filesize ?? 0);
    }

    if (mode === "director") {
      return actorNames(first)[0].localeCompare(actorNames(second)[0]);
    }

    return videoDisplayName(first).localeCompare(videoDisplayName(second));
  }

  function addedTimeValue(video: VideoFile) {
    const numericTimestamp = Number(video.added_at);

    if (Number.isFinite(numericTimestamp)) {
      return numericTimestamp;
    }

    const parsedTimestamp = Date.parse(video.added_at);
    return Number.isNaN(parsedTimestamp) ? 0 : parsedTimestamp;
  }

  function visibleVideos(videos = videoFiles) {
    return sortVideos(
      videos.filter((video) => {
        if (!matchesFilter(video)) {
          return false;
        }

        if (actorFilter !== "all" && !actorNames(video).includes(actorFilter)) {
          return false;
        }

        if (
          genreFilters.length > 0 &&
          !genreNames(video).some((genre) => genreFilters.includes(genre))
        ) {
          return false;
        }

        if (
          ratingFilters.length > 0 &&
          !ratingFilters.includes(ratingValue(video))
        ) {
          return false;
        }

        return true;
      }),
    );
  }

  function sortedListVideos(videos: VideoFile[]) {
    return [...videos].sort((first, second) => {
      const difference = compareListColumn(first, second, listSortKey);
      const directedDifference =
        listSortDirection === "asc" ? difference : -difference;

      return directedDifference || compareVideos(first, second, "name");
    });
  }

  function compareListColumn(
    first: VideoFile,
    second: VideoFile,
    key: ListSortKey,
  ) {
    if (key === "thumbnail") {
      return videoDisplayName(first).localeCompare(videoDisplayName(second));
    }

    if (key === "rating") {
      return ratingValue(first) - ratingValue(second);
    }

    if (key === "play_count") {
      return (first.play_count ?? 0) - (second.play_count ?? 0);
    }

    if (key === "explicit_content") {
      return Number(first.explicit_content) - Number(second.explicit_content);
    }

    if (key === "filesize") {
      return (first.filesize ?? 0) - (second.filesize ?? 0);
    }

    if (key === "added_at") {
      return addedTimeValue(first) - addedTimeValue(second);
    }

    return listColumnText(first, key).localeCompare(listColumnText(second, key));
  }

  function listColumnText(video: VideoFile, key: ListSortKey) {
    if (key === "title") {
      return video.title;
    }

    if (key === "filename") {
      return video.filename;
    }

    if (key === "actor") {
      return video.actor;
    }

    if (key === "genre") {
      return video.genre;
    }

    if (key === "date") {
      return video.date;
    }

    if (key === "resolution") {
      return video.resolution;
    }

    if (key === "bitrate") {
      return video.bitrate;
    }

    if (key === "backup_date") {
      return video.backup_date;
    }

    if (key === "backup_location") {
      return video.backup_location;
    }

    if (key === "notes") {
      return video.notes;
    }

    if (key === "file_path") {
      return video.file_path;
    }

    return "";
  }

  function setListColumnSort(key: ListSortKey) {
    if (listSortKey === key) {
      setListSortDirection((currentDirection) =>
        currentDirection === "asc" ? "desc" : "asc",
      );
      return;
    }

    setListSortKey(key);
    setListSortDirection("asc");
  }

  function contentVisibleVideos(videos = videoFiles) {
    return videos.filter(isVideoVisibleByExplicitSetting);
  }

  function buildGroups(
    nameGetter: (video: VideoFile) => string[],
  ): LibraryGroup[] {
    const groups = new Map<string, VideoFile[]>();

    for (const video of visibleVideos()) {
      for (const name of nameGetter(video)) {
        groups.set(name, [...(groups.get(name) ?? []), video]);
      }
    }

    return Array.from(groups.entries())
      .map<LibraryGroup>(([name, videos]) => ({
        name,
        videos,
        artworkVideo: videos[0],
      }))
      .sort((first, second) => first.name.localeCompare(second.name));
  }

  function actorGroups() {
    return buildGroups(actorNames);
  }

  function genreGroups() {
    return buildGroups(genreNames);
  }

  function ratingGroups() {
    return buildGroups(ratingGroupName).sort((first, second) => {
      const firstRating = ratingValue(first.artworkVideo);
      const secondRating = ratingValue(second.artworkVideo);

      if (firstRating === 0) {
        return secondRating === 0 ? 0 : -1;
      }

      if (secondRating === 0) {
        return 1;
      }

      return secondRating - firstRating;
    });
  }

  function selectedActorGroup() {
    return actorGroups().find((actor) => actor.name === selectedActor) ?? null;
  }

  function selectedGenreGroup() {
    return genreGroups().find((genre) => genre.name === selectedGenre) ?? null;
  }

  function selectedRatingGroup() {
    if (selectedRating === null) {
      return null;
    }

    return (
      ratingGroups().find(
        (rating) => rating.name === formatRatingLabel(selectedRating),
      ) ?? null
    );
  }

  function openVideoContextMenu(event: MouseEvent, video: VideoFile) {
    event.preventDefault();
    const rightClickedSelected = selectedVideoPaths.includes(video.file_path);
    const targetPaths = rightClickedSelected
      ? selectedVideoPaths
      : [video.file_path];
    const targetPathSet = new Set(targetPaths);
    const targetVideos = videoFiles.filter((currentVideo) =>
      targetPathSet.has(currentVideo.file_path),
    );

    setSelectedVideo(video);
    setSelectedVideoPaths(targetPaths);
    setRightPanelMode("video");
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      video,
      videos: targetVideos.length > 0 ? targetVideos : [video],
    });
  }

  function selectVideoCard(
    event: MouseEvent<HTMLElement>,
    video: VideoFile,
    selectionScopeVideos = visibleVideos(),
  ) {
    setSelectedVideo(video);
    setRightPanelMode("video");
    setIsRightPanelEditing(false);

    if (event.shiftKey) {
      setSelectedVideoPaths((currentPaths) => {
        const anchorPath = currentPaths[currentPaths.length - 1] ||
          selectionAnchorPathRef.current;
        const anchorIndex = selectionScopeVideos.findIndex(
          (currentVideo) => currentVideo.file_path === anchorPath,
        );
        const clickedIndex = selectionScopeVideos.findIndex(
          (currentVideo) => currentVideo.file_path === video.file_path,
        );

        if (anchorIndex >= 0 && clickedIndex >= 0) {
          const rangeStart = Math.min(anchorIndex, clickedIndex);
          const rangeEnd = Math.max(anchorIndex, clickedIndex);
          const rangePaths = selectionScopeVideos
            .slice(rangeStart, rangeEnd + 1)
            .map((rangeVideo) => rangeVideo.file_path);

          return event.ctrlKey || event.metaKey
            ? Array.from(new Set([...currentPaths, ...rangePaths]))
            : rangePaths;
        }

        return event.ctrlKey || event.metaKey
          ? Array.from(new Set([...currentPaths, video.file_path]))
          : [video.file_path];
      });
      return;
    }

    if (event.ctrlKey || event.metaKey) {
      setSelectedVideoPaths((currentPaths) => {
        if (currentPaths.includes(video.file_path)) {
          const nextPaths = currentPaths.filter(
            (currentPath) => currentPath !== video.file_path,
          );
          return nextPaths.length > 0 ? nextPaths : [video.file_path];
        }

        return [...currentPaths, video.file_path];
      });
      selectionAnchorPathRef.current = video.file_path;
      return;
    }

    selectionAnchorPathRef.current = video.file_path;
    setSelectedVideoPaths([video.file_path]);
  }

  function renderVideoResults(videos: VideoFile[]) {
    const filteredVideos = visibleVideos(videos);

    if (mainViewMode === "list") {
      return renderVideoList(filteredVideos);
    }

    return (
      <div className="video-grid">
        {filteredVideos.map((video) => renderVideoCard(video, filteredVideos))}
      </div>
    );
  }

  function renderArtwork(video: VideoFile, className: string) {
    if (hasCachedThumbnail(video)) {
      return <img alt="" className={className} src={artworkSource(video)} />;
    }

    return (
      <span
        className={
          hasThumbnailFailed(video)
            ? `${className} thumbnail-placeholder thumbnail-failed`
            : `${className} thumbnail-placeholder`
        }
      />
    );
  }

  function renderImagePath(path: string, className: string, refreshKey = 0) {
    const source = convertFileSrc(path);

    return (
      <img
        alt=""
        className={className}
        src={refreshKey ? `${source}?refresh=${refreshKey}` : source}
      />
    );
  }

  function renderActorArtwork(
    group: LibraryGroup,
    className: string,
    showLabel = false,
  ) {
    const actorThumbnail = actorThumbnails[group.name];
    const artwork = actorThumbnail ? (
      renderImagePath(
        actorThumbnail,
        className,
        actorThumbnailRefreshKeys[group.name],
      )
    ) : (
      <span className={`${className} actor-thumbnail-placeholder`}>
        {group.name}
      </span>
    );

    if (!showLabel) {
      return artwork;
    }

    return (
      <span className="artwork-frame right-panel-actor-frame">
        {artwork}
        <span className="thumbnail-label">{group.name}</span>
      </span>
    );
  }

  function renderSelectedVideosActorArtwork(videos: VideoFile[]) {
    const firstVideo = videos[0];

    if (!firstVideo) {
      return null;
    }

    const actorName = actorNames(firstVideo)[0];
    const actorGroup: LibraryGroup = {
      name: actorName,
      videos,
      artworkVideo: firstVideo,
    };

    return (
      <button
        className="actor-details-link"
        type="button"
        onClick={() => showActorVideosByName(actorName, actorGroup.videos)}
      >
        {renderActorArtwork(actorGroup, "actor-details-artwork", true)}
      </button>
    );
  }

  function renderVideoCard(video: VideoFile, selectionScopeVideos: VideoFile[]) {
    const isSelected = selectedVideoPaths.includes(video.file_path);

    return (
      <button
        className={isSelected ? "video-card selected" : "video-card"}
        key={video.file_path}
        type="button"
        onClick={(event) => selectVideoCard(event, video, selectionScopeVideos)}
        onContextMenu={(event) => openVideoContextMenu(event, video)}
        onDoubleClick={() => openVideo(video.file_path)}
        onMouseEnter={(event) => startHoverPreview(event, video)}
        onMouseMove={moveHoverPreview}
        onMouseLeave={stopHoverPreview}
      >
        <span className="artwork-frame">
          {renderArtwork(video, "video-artwork")}
          {showThumbnailTitles && (
            <span className="thumbnail-label">{video.title}</span>
          )}
        </span>
      </button>
    );
  }

  function renderListHeader(key: ListSortKey, label: string) {
    const isActive = listSortKey === key;

    return (
      <th>
        <button
          className={isActive ? "active" : ""}
          type="button"
          onClick={() => setListColumnSort(key)}
        >
          {label}
          {isActive && (
            <span>{listSortDirection === "asc" ? " A-Z" : " Z-A"}</span>
          )}
        </button>
      </th>
    );
  }

  function renderVideoList(videos: VideoFile[]) {
    const sortedVideos = sortedListVideos(videos);

    return (
      <div className="video-list-scroll">
        <table className="video-list-table">
          <thead>
            <tr>
              {renderListHeader("thumbnail", "Thumbnail")}
              {renderListHeader("title", "Title")}
              {renderListHeader("filename", "Filename")}
              {renderListHeader("actor", "Actor")}
              {renderListHeader("genre", "Genre")}
              {renderListHeader("date", "Date")}
              {renderListHeader("rating", "Rating")}
              {renderListHeader("play_count", "Play count")}
              {renderListHeader("explicit_content", "Explicit")}
              {renderListHeader("filesize", "Filesize")}
              {renderListHeader("resolution", "Resolution")}
              {renderListHeader("bitrate", "Bitrate")}
              {renderListHeader("added_at", "Added")}
              {renderListHeader("backup_date", "Backup date")}
              {renderListHeader("backup_location", "Backup location")}
              {renderListHeader("notes", "Notes")}
              {renderListHeader("file_path", "File path")}
            </tr>
          </thead>
          <tbody>
            {sortedVideos.map((video) => {
              const isSelected = selectedVideoPaths.includes(video.file_path);

              return (
                <tr
                  className={isSelected ? "selected" : ""}
                  key={video.file_path}
                  tabIndex={0}
                  onClick={(event) => selectVideoCard(event, video, sortedVideos)}
                  onContextMenu={(event) => openVideoContextMenu(event, video)}
                  onDoubleClick={() => openVideo(video.file_path)}
                  onMouseEnter={(event) => startHoverPreview(event, video)}
                  onMouseMove={moveHoverPreview}
                  onMouseLeave={stopHoverPreview}
                >
                  <td>
                    <span className="video-list-thumbnail">
                      {renderArtwork(video, "video-artwork")}
                    </span>
                  </td>
                  <td>{textOrDash(video.title)}</td>
                  <td>{textOrDash(video.filename)}</td>
                  <td>{textOrDash(video.actor)}</td>
                  <td>{textOrDash(video.genre)}</td>
                  <td>{textOrDash(video.date)}</td>
                  <td>{video.rating ? `${video.rating}/10` : "Unrated"}</td>
                  <td>{video.play_count ?? 0}</td>
                  <td>{textOrDash(video.explicit_content)}</td>
                  <td>{formatFileSize(video.filesize) || "-"}</td>
                  <td>{textOrDash(video.resolution)}</td>
                  <td>{textOrDash(video.bitrate)}</td>
                  <td>{formatAddedAt(video.added_at) || "-"}</td>
                  <td>{textOrDash(video.backup_date)}</td>
                  <td>{textOrDash(video.backup_location)}</td>
                  <td>{textOrDash(video.notes)}</td>
                  <td>{textOrDash(video.file_path)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    );
  }

  function renderHoverPreview() {
    if (!hoverPreview) {
      return null;
    }

    return (
      <aside
        className={
          hoverPreview.showThumbnail
            ? "video-hover-preview with-thumbnail"
            : "video-hover-preview"
        }
        style={{
          left: hoverPreview.x + 16,
          top: hoverPreview.y + 16,
        }}
      >
        {hoverPreview.showThumbnail && (
          <span className="video-hover-artwork">
            {renderArtwork(hoverPreview.video, "video-artwork")}
          </span>
        )}
        <dl>
          <div>
            <dt>Actor</dt>
            <dd>{hoverPreview.video.actor || "-"}</dd>
          </div>
          <div>
            <dt>Rating</dt>
            <dd>{hoverPreview.video.rating ? `${hoverPreview.video.rating}/10` : "Unrated"}</dd>
          </div>
          <div>
            <dt>Play count</dt>
            <dd>{hoverPreview.video.play_count ?? 0}</dd>
          </div>
          <div>
            <dt>File size</dt>
            <dd>{formatFileSize(hoverPreview.video.filesize) || "-"}</dd>
          </div>
        </dl>
      </aside>
    );
  }

  function renderGroupCard(
    group: LibraryGroup,
    selectedName: string,
    onSelect: (group: LibraryGroup) => void,
    artworkPath = "",
  ) {
    return (
      <button
        className={
          selectedName === group.name ? "video-card selected" : "video-card"
        }
        key={group.name}
        type="button"
        onClick={() => onSelect(group)}
      >
        <span className="artwork-frame">
          {artworkPath
            ? renderImagePath(artworkPath, "video-artwork")
            : renderArtwork(group.artworkVideo, "video-artwork")}
          {showThumbnailTitles && (
            <span className="thumbnail-label">
              {group.name}
              <small>
                {group.videos.length}{" "}
                {group.videos.length === 1 ? "video" : "videos"}
              </small>
            </span>
          )}
        </span>
      </button>
    );
  }

  function renderActorGroupCard(
    group: LibraryGroup,
    selectedName: string,
    onSelect: (group: LibraryGroup) => void,
  ) {
    return (
      <button
        className={
          selectedName === group.name
            ? "video-card actor-card selected"
            : "video-card actor-card"
        }
        key={group.name}
        type="button"
        onClick={() => onSelect(group)}
      >
        <span className="artwork-frame">
          {renderActorArtwork(group, "actor-artwork")}
          {showThumbnailTitles && (
            <span className="thumbnail-label">
              {group.name}
              <small>
                {group.videos.length}{" "}
                {group.videos.length === 1 ? "video" : "videos"}
              </small>
            </span>
          )}
        </span>
      </button>
    );
  }

  function renderMainGrid() {
    if (activeView === "settings") {
      return renderSettingsPage();
    }

    if (activeView === "actors") {
      const actorGroup = selectedActorGroup();

      if (actorGroup) {
        return (
          <>
            <button
              className="back-button"
              type="button"
              onClick={() => {
                pushNavigationSnapshot();
                setSelectedActor("");
                setRightPanelMode("actor");
                setIsRightPanelEditing(false);
              }}
            >
              All Actors
            </button>
            {renderVideoResults(actorGroup.videos)}
          </>
        );
      }

      return (
        <div className="video-grid actor-grid">
          {actorGroups().map((actor) =>
            renderActorGroupCard(
              actor,
              selectedActor,
              (group) => {
                pushNavigationSnapshot();
                setSelectedActor(group.name);
                setSelectedGenre("");
                setSelectedRating(null);
                setRightPanelMode("actor");
                setIsRightPanelEditing(false);
              },
            ),
          )}
        </div>
      );
    }

    if (activeView === "genres") {
      const genreGroup = selectedGenreGroup();

      if (genreGroup) {
        return (
          <>
            <button
              className="back-button"
              type="button"
              onClick={() => {
                pushNavigationSnapshot();
                setSelectedGenre("");
                setRightPanelMode("genre");
                setIsRightPanelEditing(false);
              }}
            >
              All Genres
            </button>
            {renderVideoResults(genreGroup.videos)}
          </>
        );
      }

      return (
        <div className="video-grid">
          {genreGroups().map((genre) =>
            renderGroupCard(genre, selectedGenre, (group) => {
              pushNavigationSnapshot();
              setSelectedGenre(group.name);
              setSelectedActor("");
              setSelectedRating(null);
              setRightPanelMode("genre");
              setIsRightPanelEditing(false);
            }),
          )}
        </div>
      );
    }

    if (activeView === "ratings") {
      const ratingGroup = selectedRatingGroup();

      if (ratingGroup) {
        return (
          <>
            <button
              className="back-button"
              type="button"
              onClick={() => {
                pushNavigationSnapshot();
                setSelectedRating(null);
                setRightPanelMode("rating");
                setIsRightPanelEditing(false);
              }}
            >
              All Ratings
            </button>
            {renderVideoResults(ratingGroup.videos)}
          </>
        );
      }

      return (
        <div className="video-grid">
          {ratingGroups().map((rating) =>
            renderGroupCard(
              rating,
              selectedRating === null ? "" : formatRatingLabel(selectedRating),
              (group) => {
                pushNavigationSnapshot();
                setSelectedRating(ratingValue(group.artworkVideo));
                setSelectedActor("");
                setSelectedGenre("");
                setRightPanelMode("rating");
                setIsRightPanelEditing(false);
              },
            ),
          )}
        </div>
      );
    }

    return renderVideoResults(videoFiles);
  }

  function renderSettingsPage() {
    return (
      <section className="settings-page">
        <div className="settings-section">
          <h2>Settings</h2>
          <div className="settings-field">
            <label htmlFor="thumbnail-frame-second">
              Thumbnail Generation Time On Video
            </label>
            <input
              id="thumbnail-frame-second"
              min="0"
              step="1"
              type="number"
              value={thumbnailFrameSecond}
              onChange={(event) => setThumbnailFrameSecond(event.target.value)}
            />
          </div>
          <div className="settings-field">
            <label htmlFor="thumbnail-size">Thumbnail size</label>
            <div className="thumbnail-size-control">
              <input
                id="thumbnail-size"
                max="360"
                min="120"
                step="12"
                type="range"
                value={gridSize}
                onChange={(event) => updateThumbnailSize(event.target.value)}
              />
              <input
                aria-label="Thumbnail size pixels"
                max="360"
                min="120"
                step="12"
                type="number"
                value={gridSize}
                onChange={(event) => updateThumbnailSize(event.target.value)}
              />
              <span>{gridSize}px</span>
            </div>
          </div>
          <div className="settings-field">
            <label htmlFor="font-size">Font size</label>
            <select
              id="font-size"
              value={appFontSize}
              onChange={(event) =>
                setAppFontSize(event.target.value as AppFontSize)
              }
            >
              <option value="small">Small</option>
              <option value="medium">Medium</option>
              <option value="large">Large</option>
            </select>
          </div>
          <div className="settings-field">
            <label className="settings-checkbox" htmlFor="show-thumbnail-titles">
              <input
                checked={showThumbnailTitles}
                id="show-thumbnail-titles"
                type="checkbox"
                onChange={(event) =>
                  setShowThumbnailTitles(event.target.checked)
                }
              />
              Show titles on thumbnails
            </label>
          </div>
          <div className="settings-field">
            <label className="settings-checkbox" htmlFor="hide-explicit-content">
              <input
                checked={hideExplicitContent}
                id="hide-explicit-content"
                type="checkbox"
                onChange={(event) =>
                  toggleHideExplicitContent(event.target.checked)
                }
              />
              Hide explicit content
            </label>
          </div>
          <button type="button" onClick={setExplicitPassword}>
            {explicitContentPasswordHash
              ? "Change Explicit Password"
              : "Set Explicit Password"}
          </button>
          <button disabled type="button">
            Select Database JSON
          </button>
          <button type="button" onClick={exportDatabaseFile}>
            Export Database JSON
          </button>
          <button type="button" onClick={addDirectory}>
            Add Directory
          </button>
          <button type="button" onClick={() => setIsStatisticsOpen(true)}>
            Statistics
          </button>
          <button
            type="button"
            disabled={isCheckingForUpdate || isInstallingUpdate}
            onClick={checkForAppUpdates}
          >
            {isCheckingForUpdate ? "Checking..." : "Check for Updates"}
          </button>
          {pendingUpdate && (
            <button
              type="button"
              disabled={isInstallingUpdate}
              onClick={installPendingUpdate}
            >
              {isInstallingUpdate ? "Installing..." : "Install Update"}
            </button>
          )}
          {updateStatus && <p>{updateStatus}</p>}
          {updateDownloadProgress && <p>{updateDownloadProgress}</p>}
          <button type="button" onClick={previewRestoreBackup}>
            Restore From Backup
          </button>
          <button
            type="button"
            disabled={isRefreshingDatabase || selectedFolders.length === 0}
            onClick={refreshDatabase}
          >
            {isRefreshingDatabase ? "Refreshing..." : "Refresh All Database"}
          </button>
        </div>

        <div className="settings-section">
          <h2>Library</h2>
          <p>Total videos: {videoFiles.length}</p>
          <p>Total file size: {formatFileSize(totalVideoSize()) || "-"}</p>
          <p>Database: {databasePath || "-"}</p>
        </div>

        <div className="settings-section">
          <h2>Directories</h2>
          {selectedFolders.length > 0 ? (
            <ul className="directory-list">
              {selectedFolders.map((folder) => (
                <li key={folder}>{folder}</li>
              ))}
            </ul>
          ) : (
            <p>No directories added.</p>
          )}
        </div>
      </section>
    );
  }

  function refreshReportChangeCount(report: RefreshReport) {
    return (
      report.changed_files.length +
      report.added_files.length +
      report.removed_files.length
    );
  }

  function sortedStatisticEntries(counts: Record<string, number>) {
    return Object.entries(counts).sort((first, second) => {
      const countDifference = second[1] - first[1];
      return countDifference || first[0].localeCompare(second[0]);
    });
  }

  async function resetStatistics() {
    setErrorMessage("");

    try {
      const statistics = await invoke<WatchStatistics>("reset_watch_statistics");
      setWatchStatistics(normalizeWatchStatistics(statistics));
    } catch (error) {
      setErrorMessage(String(error));
    }
  }

  function renderStatisticTable(title: string, counts: Record<string, number>) {
    const entries = sortedStatisticEntries(counts);

    return (
      <div className="refresh-section">
        <h3>{title}</h3>
        {entries.length === 0 ? (
          <p>No watches yet.</p>
        ) : (
          <table className="statistics-table">
            <tbody>
              {entries.map(([name, count]) => (
                <tr key={name}>
                  <td>{name}</td>
                  <td>{count}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    );
  }

  function renderStatisticsModal() {
    if (!isStatisticsOpen) {
      return null;
    }

    return (
      <div className="modal-backdrop">
        <section className="refresh-dialog" aria-modal="true" role="dialog">
          <div className="refresh-header">
            <h2>Watch Statistics</h2>
            <p>Total watched: {watchStatistics.total_watches}</p>
          </div>

          {renderStatisticTable("Actors", watchStatistics.actor_counts)}
          {renderStatisticTable("Genres", watchStatistics.genre_counts)}
          {renderStatisticTable("Years", watchStatistics.year_counts)}

          <div className="refresh-actions">
            <button type="button" onClick={resetStatistics}>
              Reset Statistics
            </button>
            <button type="button" onClick={() => setIsStatisticsOpen(false)}>
              Close
            </button>
          </div>
        </section>
      </div>
    );
  }

  function renderBackupResultModal() {
    if (!backupResult) {
      return null;
    }

    return (
      <div className="modal-backdrop">
        <section className="refresh-dialog" aria-modal="true" role="dialog">
          <div className="refresh-header">
            <h2>Backup Complete</h2>
            <p>{backupResult.backed_up_count} videos added to backup.</p>
            <p>{backupResult.zip_path}</p>
          </div>

          {backupResult.errors.length > 0 && (
            <div className="refresh-section">
              <h3>Errors</h3>
              <ul className="refresh-path-list">
                {backupResult.errors.map((error) => (
                  <li key={error}>{error}</li>
                ))}
              </ul>
            </div>
          )}

          <div className="refresh-actions">
            <button type="button" onClick={() => setBackupResult(null)}>
              Close
            </button>
          </div>
        </section>
      </div>
    );
  }

  function renderRestorePreviewModal() {
    if (!restorePreview) {
      return null;
    }

    const restorableItems = restorePreview.items.filter(
      (item) => !item.exists && !item.error,
    );
    const existingItems = restorePreview.items.filter((item) => item.exists);

    return (
      <div className="modal-backdrop">
        <section className="refresh-dialog" aria-modal="true" role="dialog">
          <div className="refresh-header">
            <h2>Restore Preview</h2>
            <p>{restorePreview.zip_path}</p>
            <p>
              {restorableItems.length} files ready, {existingItems.length} existing
              files will be skipped.
            </p>
          </div>

          <div className="refresh-section">
            <h3>Files</h3>
            <div className="refresh-list">
              {restorePreview.items.map((item) => (
                <div className="refresh-file" key={`${item.filename}-${item.target_path}`}>
                  <strong>{item.filename}</strong>
                  <span>{item.target_path}</span>
                  {item.exists && (
                    <span className="organize-warning">
                      Already exists. Restore will skip this file.
                    </span>
                  )}
                  {item.error && <span className="organize-error">{item.error}</span>}
                </div>
              ))}
            </div>
          </div>

          <div className="refresh-actions">
            <button type="button" onClick={() => setRestorePreview(null)}>
              Cancel
            </button>
            <button
              type="button"
              disabled={restorableItems.length === 0 || isRestoring}
              onClick={confirmRestoreBackup}
            >
              Restore Missing Files
            </button>
          </div>
        </section>
      </div>
    );
  }

  function renderRestoreResultModal() {
    if (!restoreResult) {
      return null;
    }

    return (
      <div className="modal-backdrop">
        <section className="refresh-dialog" aria-modal="true" role="dialog">
          <div className="refresh-header">
            <h2>Restore Complete</h2>
            <p>{restoreResult.restored_count} files restored.</p>
            <p>{restoreResult.skipped_count} existing files skipped.</p>
          </div>

          {restoreResult.errors.length > 0 && (
            <div className="refresh-section">
              <h3>Errors</h3>
              <ul className="refresh-path-list">
                {restoreResult.errors.map((error) => (
                  <li key={error}>{error}</li>
                ))}
              </ul>
            </div>
          )}

          <div className="refresh-actions">
            <button type="button" onClick={() => setRestoreResult(null)}>
              Close
            </button>
          </div>
        </section>
      </div>
    );
  }

  function renderDeleteConfirmationModal() {
    if (pendingDeleteVideos.length === 0) {
      return null;
    }

    return (
      <div className="modal-backdrop">
        <section className="refresh-dialog" aria-modal="true" role="dialog">
          <div className="refresh-header">
            <h2>Move Videos To Recycle Bin?</h2>
            <p>
              This will move the selected video file(s) to the Windows Recycle Bin
              and remove them from the JSON database.
            </p>
            <p>{pendingDeleteVideos.length} file(s) selected.</p>
          </div>

          <div className="refresh-section">
            <h3>Selected Files</h3>
            <ul className="refresh-path-list">
              {pendingDeleteVideos.map((video) => (
                <li key={video.file_path}>{video.filename}</li>
              ))}
            </ul>
          </div>

          <div className="refresh-actions">
            <span>Files can be restored from the Windows Recycle Bin.</span>
            <button type="button" onClick={() => setPendingDeleteVideos([])}>
              Cancel
            </button>
            <button type="button" onClick={confirmDeleteSelectedVideoFiles}>
              Confirm
            </button>
          </div>
        </section>
      </div>
    );
  }

  function renderFileOperationProgressModal(
    title: string,
    progress: FileOperationProgress | null,
    isActive: boolean,
  ) {
    if (!progress || !isActive) {
      return null;
    }

    const progressPercent =
      progress.total > 0
        ? Math.round((progress.completed / progress.total) * 100)
        : 0;

    return (
      <div className="modal-backdrop">
        <section className="file-progress-dialog" aria-modal="true" role="dialog">
          <div className="refresh-header">
            <h2>{title}</h2>
            <p>{progress.current_file}</p>
          </div>
          <div className="organize-progress-bar" aria-hidden="true">
            <span style={{ width: `${progressPercent}%` }} />
          </div>
          <p>
            {progress.completed} / {progress.total} files
          </p>
        </section>
      </div>
    );
  }

  function renderRefreshReportModal() {
    if (!refreshReport) {
      return null;
    }

    const totalChanges = refreshReportChangeCount(refreshReport);

    return (
      <div className="modal-backdrop">
        <section className="refresh-dialog" aria-modal="true" role="dialog">
          <div className="refresh-header">
            <h2>Database Refresh Report</h2>
            <p>Not editing any files. Only the database JSON was updated.</p>
            <p>
              {totalChanges === 0
                ? "No file tag changes found."
                : `${totalChanges} database updates found.`}
            </p>
          </div>

          {refreshReport.changed_files.length > 0 && (
            <div className="refresh-section">
              <h3>Changed Tags</h3>
              <div className="refresh-list">
                {refreshReport.changed_files.map((file) => (
                  <div className="refresh-file" key={file.file_path}>
                    <strong>{file.filename}</strong>
                    <span>{file.file_path}</span>
                    {file.changes.map((change) => (
                      <div
                        className="refresh-field-change"
                        key={`${file.file_path}-${change.field}`}
                      >
                        <b>{change.field}</b>
                        <span>{change.old_value || "-"}</span>
                        <span>{change.new_value || "-"}</span>
                      </div>
                    ))}
                  </div>
                ))}
              </div>
            </div>
          )}

          {refreshReport.added_files.length > 0 && (
            <div className="refresh-section">
              <h3>Added Files</h3>
              <ul className="refresh-path-list">
                {refreshReport.added_files.map((filePath) => (
                  <li key={filePath}>{filePath}</li>
                ))}
              </ul>
            </div>
          )}

          {refreshReport.removed_files.length > 0 && (
            <div className="refresh-section">
              <h3>Removed From Database</h3>
              <ul className="refresh-path-list">
                {refreshReport.removed_files.map((filePath) => (
                  <li key={filePath}>{filePath}</li>
                ))}
              </ul>
            </div>
          )}

          <div className="refresh-actions">
            <button type="button" onClick={() => setRefreshReport(null)}>
              Close
            </button>
          </div>
        </section>
      </div>
    );
  }

  function renderOrganizePreviewModal() {
    if (!organizePreview) {
      return null;
    }

    const validItems = organizePreview.items.filter((item) => !item.error);
    const renamedItems = organizePreview.items.filter((item) => item.renamed);

    return (
      <div className="modal-backdrop">
        <section className="refresh-dialog" aria-modal="true" role="dialog">
          <div className="refresh-header">
            <h2>Organize Files Preview</h2>
            <p>This will copy files first, then update only the database JSON paths.</p>
            <p>Pattern: {organizePreview.pattern}</p>
            <p>Base folder: {organizePreview.base_folder}</p>
          </div>

          {organizePreview.folders_to_create.length > 0 && (
            <div className="refresh-section">
              <h3>Folders To Create</h3>
              <ul className="refresh-path-list">
                {organizePreview.folders_to_create.map((folder) => (
                  <li key={folder}>{folder}</li>
                ))}
              </ul>
            </div>
          )}

          <div className="refresh-section">
            <h3>Files</h3>
            <div className="refresh-list">
              {organizePreview.items.map((item) => (
                <div className="refresh-file organize-file" key={item.file_path}>
                  <strong>{item.filename}</strong>
                  <div className="organize-path-row">
                    <b>Current</b>
                    <span>{item.file_path}</span>
                  </div>
                  <div className="organize-path-row">
                    <b>New</b>
                    <span>{item.target_path}</span>
                  </div>
                  {item.renamed && (
                    <span className="organize-warning">
                      Auto-renamed because a file with that name already exists.
                    </span>
                  )}
                  {item.error && (
                    <span className="organize-error">{item.error}</span>
                  )}
                </div>
              ))}
            </div>
          </div>

          <div className="refresh-actions">
            <span>
              {validItems.length} ready, {renamedItems.length} renamed,{" "}
              {organizePreview.items.length - validItems.length} blocked
            </span>
            <button
              type="button"
              onClick={() => setOrganizePreview(null)}
              disabled={isOrganizing}
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={confirmOrganizeVideos}
              disabled={isOrganizing || validItems.length === 0}
            >
              {isOrganizing ? "Copying..." : "Confirm Copy"}
            </button>
          </div>
        </section>
      </div>
    );
  }

  function renderOrganizeResultModal() {
    if (!organizeResult) {
      return null;
    }

    const renamedItems = organizeResult.items.filter((item) => item.renamed);

    return (
      <div className="modal-backdrop">
        <section className="refresh-dialog" aria-modal="true" role="dialog">
          <div className="refresh-header">
            <h2>Organize Complete</h2>
            <p>
              Copied {organizeResult.copied_count} files, updated{" "}
              {organizeResult.updated_count} database rows.
            </p>
            <p>{organizeResult.renamed_count} files were auto-renamed.</p>
          </div>

          {renamedItems.length > 0 && (
            <div className="refresh-section">
              <h3>Auto-Renamed</h3>
              <ul className="refresh-path-list">
                {renamedItems.map((item) => (
                  <li key={`${item.file_path}-${item.target_path}`}>
                    {item.target_path}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {organizeResult.errors.length > 0 && (
            <div className="refresh-section">
              <h3>Errors</h3>
              <ul className="refresh-path-list">
                {organizeResult.errors.map((error) => (
                  <li key={error}>{error}</li>
                ))}
              </ul>
            </div>
          )}

          <div className="refresh-actions">
            <button type="button" onClick={() => setOrganizeResult(null)}>
              Close
            </button>
          </div>
        </section>
      </div>
    );
  }

  function renderOrganizeProgressPanel() {
    if (!organizeProgress || !isOrganizing) {
      return null;
    }

    const progressPercent =
      organizeProgress.total > 0
        ? Math.round(
            (organizeProgress.completed / organizeProgress.total) * 100,
          )
        : 0;

    return (
      <section
        className={
          isOrganizeProgressMinimized
            ? "organize-progress-panel minimized"
            : "organize-progress-panel"
        }
        aria-live="polite"
      >
        <div className="organize-progress-header">
          <h2>Copying Files</h2>
          <button
            type="button"
            onClick={() =>
              setIsOrganizeProgressMinimized((currentValue) => !currentValue)
            }
          >
            {isOrganizeProgressMinimized ? "Show" : "Minimize"}
          </button>
        </div>
        {!isOrganizeProgressMinimized && (
          <>
            <div className="organize-progress-bar" aria-hidden="true">
              <span style={{ width: `${progressPercent}%` }} />
            </div>
            <p>
              {organizeProgress.completed} / {organizeProgress.total} files
            </p>
            <p>{organizeProgress.current_file}</p>
          </>
        )}
      </section>
    );
  }

  function renderRightPanelDetailToggle() {
    return (
      <div className="detail-mode-toggle" aria-label="Right panel detail mode">
        <button
          className={rightPanelDetailMode === "short" ? "active" : ""}
          type="button"
          onClick={() => setRightPanelDetailMode("short")}
        >
          Short
        </button>
        <button
          className={rightPanelDetailMode === "extended" ? "active" : ""}
          type="button"
          onClick={() => setRightPanelDetailMode("extended")}
        >
          Extended
        </button>
      </div>
    );
  }

  function renderAutocompleteInput(
    field: AutocompleteField,
    label: string,
    id: string,
  ) {
    const options = filteredAutocompleteOptions(field, editForm[field]);
    const isOpen = autocompleteState?.field === field && options.length > 0;
    const activeIndex = Math.min(
      autocompleteState?.field === field ? autocompleteState.activeIndex : 0,
      Math.max(0, options.length - 1),
    );

    return (
      <div className="autocomplete-field">
        <label htmlFor={id}>{label}</label>
        <input
          autoComplete="off"
          id={id}
          value={editForm[field]}
          onBlur={() => {
            window.setTimeout(() => setAutocompleteState(null), 120);
          }}
          onChange={(event) => {
            setEditForm((currentForm) => ({
              ...currentForm,
              [field]: event.target.value,
            }));
            setAutocompleteState({ field, activeIndex: 0 });
          }}
          onFocus={() => setAutocompleteState({ field, activeIndex: 0 })}
          onKeyDown={(event) =>
            handleAutocompleteKeyDown(event, field, editForm[field])
          }
        />
        {isOpen && (
          <div className="autocomplete-menu" role="listbox">
            {options.map((option, index) => (
              <button
                className={index === activeIndex ? "active" : ""}
                key={option}
                type="button"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => applyAutocompleteSuggestion(field, option)}
              >
                {option}
              </button>
            ))}
          </div>
        )}
      </div>
    );
  }

  function renderReadOnlyRow(label: string, value: string | number) {
    return (
      <div>
        <dt>{label}</dt>
        <dd>{value === "" ? "-" : value}</dd>
      </div>
    );
  }

  function renderVideoReadOnlyDetails(
    videoValues: VideoEditForm,
    extended: boolean,
    includeFileSpecificRows = false,
  ) {
    return (
      <dl className="tag-list">
        {extended && includeFileSpecificRows && selectedVideo && (
          <>
            {renderReadOnlyRow("Filename", selectedVideo.filename)}
            {renderReadOnlyRow("File path", selectedVideo.file_path)}
          </>
        )}
        {renderReadOnlyRow("Actor", videoValues.actor)}
        {renderReadOnlyRow("Title", videoValues.title)}
        {renderReadOnlyRow("Genre", videoValues.genre)}
        {extended && renderReadOnlyRow("Date", videoValues.date)}
        {extended && renderReadOnlyRow("Backup date", videoValues.backup_date)}
        {extended &&
          renderReadOnlyRow("Backup location", videoValues.backup_location)}
        {extended && renderReadOnlyRow("Notes", videoValues.notes)}
        {renderReadOnlyRow(
          "Explicit content",
          videoValues.explicit_content === "various"
            ? "various"
            : videoValues.explicit_content === "true"
              ? "Yes"
              : "No",
        )}
        {renderReadOnlyRow(
          "Rating",
          videoValues.rating === "various"
            ? "various"
            : `${videoValues.rating}/10`,
        )}
        {extended && includeFileSpecificRows && selectedVideo &&
          renderReadOnlyRow("Filesize", formatFileSize(selectedVideo.filesize) || "-")}
        {extended &&
          includeFileSpecificRows &&
          selectedVideo &&
          renderReadOnlyRow("Resolution", selectedVideo.resolution || "-")}
        {extended &&
          includeFileSpecificRows &&
          selectedVideo &&
          renderReadOnlyRow("Bitrate", selectedVideo.bitrate || "-")}
        {renderReadOnlyRow("Play count", videoValues.play_count)}
      </dl>
    );
  }

  function renderActorSocialReadOnly(actorName: string) {
    const socialLinks = actorSocialValues(actorName);

    return (
      <dl className="tag-list">
        {renderReadOnlyRow("Website", socialLinks.website)}
        {renderReadOnlyRow("IMDb", socialLinks.imdb)}
        {renderReadOnlyRow("Instagram", socialLinks.instagram)}
        {renderReadOnlyRow("X/Twitter", socialLinks.x)}
        {renderReadOnlyRow("YouTube", socialLinks.youtube)}
      </dl>
    );
  }

  function renderRightPanel() {
    if (rightPanelMode === "actor") {
      const actorGroup = selectedActorGroup();

      if (!actorGroup) {
        return <p>Select an actor.</p>;
      }

      if (!isRightPanelEditing) {
        return (
          <>
            {renderActorArtwork(actorGroup, "actor-details-artwork", true)}
            <div className="summary-block">
              <h2>{actorGroup.name}</h2>
              <p>{actorGroup.videos.length} videos</p>
              <p>{actorBios[actorGroup.name] || "Bio not added yet."}</p>
            </div>
            {renderActorSocialReadOnly(actorGroup.name)}
            <button type="button" onClick={() => setIsRightPanelEditing(true)}>
              Edit
            </button>
          </>
        );
      }

      return (
        <>
          {renderActorArtwork(actorGroup, "actor-details-artwork", true)}
          <button
            className="thumbnail-edit-button"
            type="button"
            onClick={() => selectActorThumbnail(actorGroup.name)}
          >
            Change Thumbnail
          </button>
          <div className="summary-block">
            <h2>{actorGroup.name}</h2>
            <p>{actorGroup.videos.length} videos</p>
            <label className="actor-bio-editor" htmlFor="actor-bio">
              Bio
              <textarea
                id="actor-bio"
                value={actorBios[actorGroup.name] ?? ""}
                onChange={(event) =>
                  setActorBios((currentBios) => ({
                    ...currentBios,
                    [actorGroup.name]: event.target.value,
                  }))
                }
              />
            </label>
            <div className="actor-social-editor">
              <label htmlFor="actor-website">
                Website
                <input
                  id="actor-website"
                  value={actorSocialValues(actorGroup.name).website}
                  onChange={(event) =>
                    setActorSocialLinks((currentLinks) => ({
                      ...currentLinks,
                      [actorGroup.name]: {
                        ...actorSocialValues(actorGroup.name),
                        website: event.target.value,
                      },
                    }))
                  }
                />
              </label>
              <label htmlFor="actor-imdb">
                IMDb
                <input
                  id="actor-imdb"
                  value={actorSocialValues(actorGroup.name).imdb}
                  onChange={(event) =>
                    setActorSocialLinks((currentLinks) => ({
                      ...currentLinks,
                      [actorGroup.name]: {
                        ...actorSocialValues(actorGroup.name),
                        imdb: event.target.value,
                      },
                    }))
                  }
                />
              </label>
              <label htmlFor="actor-instagram">
                Instagram
                <input
                  id="actor-instagram"
                  value={actorSocialValues(actorGroup.name).instagram}
                  onChange={(event) =>
                    setActorSocialLinks((currentLinks) => ({
                      ...currentLinks,
                      [actorGroup.name]: {
                        ...actorSocialValues(actorGroup.name),
                        instagram: event.target.value,
                      },
                    }))
                  }
                />
              </label>
              <label htmlFor="actor-x">
                X/Twitter
                <input
                  id="actor-x"
                  value={actorSocialValues(actorGroup.name).x}
                  onChange={(event) =>
                    setActorSocialLinks((currentLinks) => ({
                      ...currentLinks,
                      [actorGroup.name]: {
                        ...actorSocialValues(actorGroup.name),
                        x: event.target.value,
                      },
                    }))
                  }
                />
              </label>
              <label htmlFor="actor-youtube">
                YouTube
                <input
                  id="actor-youtube"
                  value={actorSocialValues(actorGroup.name).youtube}
                  onChange={(event) =>
                    setActorSocialLinks((currentLinks) => ({
                      ...currentLinks,
                      [actorGroup.name]: {
                        ...actorSocialValues(actorGroup.name),
                        youtube: event.target.value,
                      },
                    }))
                  }
                />
              </label>
            </div>
            <button type="button" onClick={() => saveActorProfile(actorGroup.name)}>
              Save Profile
            </button>
          </div>
        </>
      );
    }

    if (rightPanelMode === "genre") {
      const genreGroup = selectedGenreGroup();

      if (!genreGroup) {
        return <p>Select a genre.</p>;
      }

      return (
        <>
          {renderArtwork(genreGroup.artworkVideo, "details-artwork")}
          <div className="summary-block">
            <h2>{genreGroup.name}</h2>
            <p>{genreGroup.videos.length} videos</p>
            <p>Description not added yet.</p>
          </div>
        </>
      );
    }

    if (rightPanelMode === "rating") {
      const ratingGroup = selectedRatingGroup();

      if (!ratingGroup) {
        return <p>Select a rating.</p>;
      }

      return (
        <>
          {renderArtwork(ratingGroup.artworkVideo, "details-artwork")}
          <div className="summary-block">
            <h2>{ratingGroup.name}</h2>
            {renderRatingStars(ratingValue(ratingGroup.artworkVideo))}
            <p>{ratingGroup.videos.length} videos</p>
          </div>
        </>
      );
    }

    const selectedVideos = selectedVideosForEdit();
    const isExtendedRightPanel = rightPanelDetailMode === "extended";

    if (selectedVideos.length > 1) {
      if (!isRightPanelEditing) {
        return (
          <>
            {renderRightPanelDetailToggle()}
            {renderSelectedVideosActorArtwork(selectedVideos)}
            <div className="summary-block">
              <h2>{selectedVideos.length} videos selected</h2>
              <p>Shared values are shown. Different values show as various.</p>
            </div>
            {renderVideoReadOnlyDetails(editForm, isExtendedRightPanel)}
            <button type="button" onClick={() => setIsRightPanelEditing(true)}>
              Edit
            </button>
          </>
        );
      }

      return (
        <>
          {renderRightPanelDetailToggle()}
          {renderSelectedVideosActorArtwork(selectedVideos)}
          <div className="summary-block">
            <h2>{selectedVideos.length} videos selected</h2>
            <p>Shared values are shown. Different values show as various.</p>
          </div>

          <div className="edit-form">
            <div>
              <label htmlFor="edit-title">Title</label>
              <input
                id="edit-title"
                value={editForm.title}
                onChange={(event) =>
                  setEditForm((currentForm) => ({
                    ...currentForm,
                    title: event.target.value,
                  }))
                }
              />
            </div>
            {renderAutocompleteInput("actor", "Actor", "edit-actor")}
            {renderAutocompleteInput("genre", "Genre", "edit-genre")}
            {isExtendedRightPanel && (
              <>
                <div>
                  <label htmlFor="edit-date">Date</label>
                  <input
                    id="edit-date"
                    value={editForm.date}
                    onChange={(event) =>
                      setEditForm((currentForm) => ({
                        ...currentForm,
                        date: event.target.value,
                      }))
                    }
                  />
                </div>
                <div>
                  <label htmlFor="edit-backup-date">Backup date</label>
                  <input
                    id="edit-backup-date"
                    value={editForm.backup_date}
                    onChange={(event) =>
                      setEditForm((currentForm) => ({
                        ...currentForm,
                        backup_date: event.target.value,
                      }))
                    }
                  />
                </div>
                <div>
                  <label htmlFor="edit-backup-location">Backup location</label>
                  <input
                    id="edit-backup-location"
                    value={editForm.backup_location}
                    onChange={(event) =>
                      setEditForm((currentForm) => ({
                        ...currentForm,
                        backup_location: event.target.value,
                      }))
                    }
                  />
                </div>
                <div>
                  <label htmlFor="edit-notes">Notes</label>
                  <textarea
                    id="edit-notes"
                    value={editForm.notes}
                    onChange={(event) =>
                      setEditForm((currentForm) => ({
                        ...currentForm,
                        notes: event.target.value,
                      }))
                    }
                  />
                </div>
              </>
            )}
            <label className="checkbox-field" htmlFor="edit-explicit-content-multi">
              <input
                checked={editForm.explicit_content === "true"}
                id="edit-explicit-content-multi"
                ref={(input) => {
                  if (input) {
                    input.indeterminate = editForm.explicit_content === "various";
                  }
                }}
                type="checkbox"
                onChange={(event) =>
                  setEditForm((currentForm) => ({
                    ...currentForm,
                    explicit_content: event.target.checked ? "true" : "false",
                  }))
                }
              />
              Explicit content
            </label>
            <div>
              <label htmlFor="edit-rating">Rating</label>
              <div className="rating-editor">
                {editForm.rating === "various" ? (
                  <p>various</p>
                ) : (
                  renderRatingStars(Number.parseInt(editForm.rating, 10) || 0)
                )}
                <div className="number-stepper">
                  <button
                    aria-label="Decrease rating"
                    type="button"
                    onClick={() => updateNumberField("rating", -1, 10)}
                  >
                    -
                  </button>
                  <input
                    id="edit-rating"
                    max="10"
                    min="0"
                    type={editForm.rating === "various" ? "text" : "number"}
                    value={editForm.rating}
                    onChange={(event) =>
                      setEditForm((currentForm) => ({
                        ...currentForm,
                        rating: String(
                          clampRating(
                            Number.parseInt(event.target.value, 10) || 0,
                          ),
                        ),
                      }))
                    }
                  />
                  <button
                    aria-label="Increase rating"
                    type="button"
                    onClick={() => updateNumberField("rating", 1, 10)}
                  >
                    +
                  </button>
                </div>
              </div>
            </div>
            <div>
              <label htmlFor="edit-play-count">Played count</label>
              <div className="number-stepper">
                <button
                  aria-label="Decrease played count"
                  type="button"
                  onClick={() => updateNumberField("play_count", -1)}
                >
                  -
                </button>
                <input
                  id="edit-play-count"
                  min="0"
                  type={editForm.play_count === "various" ? "text" : "number"}
                  value={editForm.play_count}
                  onChange={(event) =>
                    setEditForm((currentForm) => ({
                      ...currentForm,
                      play_count: String(
                        Math.max(0, Number.parseInt(event.target.value, 10) || 0),
                      ),
                    }))
                  }
                />
                <button
                  aria-label="Increase played count"
                  type="button"
                  onClick={() => updateNumberField("play_count", 1)}
                >
                  +
                </button>
              </div>
            </div>
          </div>

          <button type="button" onClick={saveSelectedVideo}>
            Save Selected Videos
          </button>
        </>
      );
    }

    if (!selectedVideo) {
      return <p>Select a video.</p>;
    }

    if (!isRightPanelEditing) {
      return (
        <>
          {renderRightPanelDetailToggle()}
          {renderSelectedVideosActorArtwork([selectedVideo])}
          {renderVideoReadOnlyDetails(editForm, isExtendedRightPanel, true)}
          <button type="button" onClick={() => setIsRightPanelEditing(true)}>
            Edit
          </button>
          <button type="button" onClick={() => openVideo(selectedVideo.file_path)}>
            Open
          </button>
        </>
      );
    }

    return (
      <>
        {renderRightPanelDetailToggle()}
        {renderSelectedVideosActorArtwork([selectedVideo])}

        <div className="edit-form">
          {isExtendedRightPanel && (
            <>
              <div>
                <span>Filename</span>
                <p>{selectedVideo.filename}</p>
              </div>
              <div>
                <span>File path</span>
                <p>{selectedVideo.file_path}</p>
              </div>
            </>
          )}
          <div>
            <label htmlFor="edit-title">Title</label>
            <input
              id="edit-title"
              value={editForm.title}
              onChange={(event) =>
                setEditForm((currentForm) => ({
                  ...currentForm,
                  title: event.target.value,
                }))
              }
            />
          </div>
          {renderAutocompleteInput("actor", "Actor", "edit-actor")}
          {renderAutocompleteInput("genre", "Genre", "edit-genre")}
          {isExtendedRightPanel && (
            <>
              <div>
                <label htmlFor="edit-date">Date</label>
                <input
                  id="edit-date"
                  value={editForm.date}
                  onChange={(event) =>
                    setEditForm((currentForm) => ({
                      ...currentForm,
                      date: event.target.value,
                    }))
                  }
                />
              </div>
              <div>
                <label htmlFor="edit-backup-date">Backup date</label>
                <input
                  id="edit-backup-date"
                  value={editForm.backup_date}
                  onChange={(event) =>
                    setEditForm((currentForm) => ({
                      ...currentForm,
                      backup_date: event.target.value,
                    }))
                  }
                />
              </div>
              <div>
                <label htmlFor="edit-backup-location">Backup location</label>
                <input
                  id="edit-backup-location"
                  value={editForm.backup_location}
                  onChange={(event) =>
                    setEditForm((currentForm) => ({
                      ...currentForm,
                      backup_location: event.target.value,
                    }))
                  }
                />
              </div>
              <div>
                <label htmlFor="edit-notes">Notes</label>
                <textarea
                  id="edit-notes"
                  value={editForm.notes}
                  onChange={(event) =>
                    setEditForm((currentForm) => ({
                      ...currentForm,
                      notes: event.target.value,
                    }))
                  }
                />
              </div>
            </>
          )}
          <label className="checkbox-field" htmlFor="edit-explicit-content">
            <input
              checked={editForm.explicit_content === "true"}
              id="edit-explicit-content"
              type="checkbox"
              onChange={(event) =>
                setEditForm((currentForm) => ({
                  ...currentForm,
                  explicit_content: event.target.checked ? "true" : "false",
                }))
              }
            />
            Explicit content
          </label>
          <div>
            <label htmlFor="edit-rating">Rating</label>
            <div className="rating-editor">
              {renderRatingStars(Number.parseInt(editForm.rating, 10) || 0)}
              <div className="number-stepper">
                <button
                  aria-label="Decrease rating"
                  type="button"
                  onClick={() => updateNumberField("rating", -1, 10)}
                >
                  -
                </button>
                <input
                  id="edit-rating"
                  max="10"
                  min="0"
                  type="number"
                  value={editForm.rating}
                  onChange={(event) =>
                    setEditForm((currentForm) => ({
                      ...currentForm,
                      rating: String(
                        clampRating(Number.parseInt(event.target.value, 10) || 0),
                      ),
                    }))
                  }
                />
                <button
                  aria-label="Increase rating"
                  type="button"
                  onClick={() => updateNumberField("rating", 1, 10)}
                >
                  +
                </button>
              </div>
              <p>
                {Number.parseInt(editForm.rating, 10) || 0}/10,{" "}
                {formatRatingLabel(Number.parseInt(editForm.rating, 10) || 0)}
              </p>
            </div>
          </div>
          {isExtendedRightPanel && (
            <>
              <div>
                <span>Filesize</span>
                <p>{formatFileSize(selectedVideo.filesize) || "-"}</p>
              </div>
              <div>
                <span>Resolution</span>
                <p>{selectedVideo.resolution || "-"}</p>
              </div>
              <div>
                <span>Bitrate</span>
                <p>{selectedVideo.bitrate || "-"}</p>
              </div>
            </>
          )}
          <div>
            <label htmlFor="edit-play-count">Played count</label>
            <div className="number-stepper">
              <button
                aria-label="Decrease played count"
                type="button"
                onClick={() => updateNumberField("play_count", -1)}
              >
                -
              </button>
              <input
                id="edit-play-count"
                min="0"
                type="number"
                value={editForm.play_count}
                onChange={(event) =>
                  setEditForm((currentForm) => ({
                    ...currentForm,
                    play_count: String(
                      Math.max(0, Number.parseInt(event.target.value, 10) || 0),
                    ),
                  }))
                }
              />
              <button
                aria-label="Increase played count"
                type="button"
                onClick={() => updateNumberField("play_count", 1)}
              >
                +
              </button>
            </div>
          </div>
          {isExtendedRightPanel && (
            <div>
              <span>Artwork/thumbnail</span>
              <p>{selectedVideo.artwork_thumbnail || "-"}</p>
            </div>
          )}
        </div>

        <button type="button" onClick={saveSelectedVideo}>
          Save
        </button>
        {isExtendedRightPanel && (
          <button type="button" onClick={() => openVideo(selectedVideo.file_path)}>
            Open
          </button>
        )}
      </>
    );
  }

  return (
    <main className="app-layout" style={appFontSizeVars()}>
      {!hasDatabase && (
        <button type="button" onClick={selectFolder}>
          Select Folder
        </button>
      )}

      {errorMessage && <p className="error-message">{errorMessage}</p>}

      <section
        className={
          isLeftPanelCollapsed
            ? "workspace-layout left-collapsed"
            : "workspace-layout"
        }
      >
        {!isLeftPanelCollapsed && (
          <nav className="left-panel" aria-label="Library views">
            <button
              className={activeView === "all-videos" ? "nav-item active" : "nav-item"}
              type="button"
              onClick={() => {
                if (
                  activeViewRef.current === "all-videos" &&
                  (selectedActor || selectedGenre || selectedRating !== null)
                ) {
                  pushNavigationSnapshot();
                }
                navigateToView("all-videos");
                setSelectedActor("");
                setSelectedGenre("");
                setSelectedRating(null);
                setRightPanelMode("video");
                setIsRightPanelEditing(false);
              }}
            >
              All Videos
            </button>
            <button
              className={activeView === "actors" ? "nav-item active" : "nav-item"}
              type="button"
              onClick={() => {
                if (
                  activeViewRef.current === "actors" &&
                  (selectedActor || selectedGenre || selectedRating !== null)
                ) {
                  pushNavigationSnapshot();
                }
                navigateToView("actors");
                setSelectedActor("");
                setSelectedGenre("");
                setSelectedRating(null);
                setRightPanelMode("actor");
                setIsRightPanelEditing(false);
              }}
            >
              Actors
            </button>
            <button
              className={activeView === "genres" ? "nav-item active" : "nav-item"}
              type="button"
              onClick={() => {
                if (
                  activeViewRef.current === "genres" &&
                  (selectedActor || selectedGenre || selectedRating !== null)
                ) {
                  pushNavigationSnapshot();
                }
                navigateToView("genres");
                setSelectedActor("");
                setSelectedGenre("");
                setSelectedRating(null);
                setRightPanelMode("genre");
                setIsRightPanelEditing(false);
              }}
            >
              Genres
            </button>
            <button
              className={activeView === "ratings" ? "nav-item active" : "nav-item"}
              type="button"
              onClick={() => {
                if (
                  activeViewRef.current === "ratings" &&
                  (selectedActor || selectedGenre || selectedRating !== null)
                ) {
                  pushNavigationSnapshot();
                }
                navigateToView("ratings");
                setSelectedActor("");
                setSelectedGenre("");
                setSelectedRating(null);
                setRightPanelMode("rating");
                setIsRightPanelEditing(false);
              }}
            >
              Ratings
            </button>
            <button
              className={activeView === "settings" ? "nav-item active" : "nav-item"}
              type="button"
              onClick={() => {
                if (
                  activeViewRef.current === "settings" &&
                  (selectedActor || selectedGenre || selectedRating !== null)
                ) {
                  pushNavigationSnapshot();
                }
                navigateToView("settings");
                setSelectedActor("");
                setSelectedGenre("");
                setSelectedRating(null);
                setRightPanelMode("video");
                setIsRightPanelEditing(false);
              }}
            >
              Settings
            </button>
          </nav>
        )}
        <button
          aria-label={isLeftPanelCollapsed ? "Show left panel" : "Hide left panel"}
          className="panel-divider left-divider"
          type="button"
          onClick={() => setIsLeftPanelCollapsed((collapsed) => !collapsed)}
        />

        <section className="content-shell">
          {activeView !== "settings" && (
            <div className="top-panel">
            <div className="top-panel-left">
              <button
                className="back-button"
                type="button"
                onClick={goBackView}
              >
                Back
              </button>
              <div className="filter-controls">
              <label htmlFor="filter-videos">
                Filter
                <input
                  id="filter-videos"
                  value={filterText}
                  onChange={(event) => setFilterText(event.target.value)}
                />
              </label>
              <label htmlFor="filter-actor">
                Actor
                <select
                  id="filter-actor"
                  value={actorFilter}
                  onChange={(event) => setActorFilter(event.target.value)}
                >
                  <option value="all">All actors</option>
                  {actorFilterOptions().map((actor) => (
                    <option key={actor} value={actor}>
                      {actor}
                    </option>
                  ))}
                </select>
              </label>
              <details className="multi-filter">
                <summary>{genreFilterLabel()}</summary>
                <div className="multi-filter-menu">
                  <button type="button" onClick={() => setGenreFilters([])}>
                    All genres
                  </button>
                  {genreFilterOptions().map((genre) => (
                    <label key={genre}>
                      <input
                        checked={genreFilters.includes(genre)}
                        type="checkbox"
                        onChange={() => toggleGenreFilter(genre)}
                      />
                      {genre}
                    </label>
                  ))}
                </div>
              </details>
              <details className="multi-filter">
                <summary>{ratingFilterLabel()}</summary>
                <div className="multi-filter-menu">
                  <button type="button" onClick={() => setRatingFilters([])}>
                    All ratings
                  </button>
                  {ratingFilterOptions().map((rating) => (
                    <label key={rating}>
                      <input
                        checked={ratingFilters.includes(rating)}
                        type="checkbox"
                        onChange={() => toggleRatingFilter(rating)}
                      />
                      {formatRatingLabel(rating)}
                    </label>
                  ))}
                </div>
                  </details>
                </div>
            </div>
            <div className="sort-controls">
              <div className="view-mode-toggle" aria-label="Video view mode">
                <button
                  className={mainViewMode === "grid" ? "active" : ""}
                  type="button"
                  onClick={() => setMainViewMode("grid")}
                >
                  Grid
                </button>
                <button
                  className={mainViewMode === "list" ? "active" : ""}
                  type="button"
                  onClick={() => setMainViewMode("list")}
                >
                  List
                </button>
              </div>
              <label htmlFor="sort-videos">
                Sort
                <select
                  id="sort-videos"
                  value={sortMode}
                  onChange={(event) =>
                    setSortMode(event.target.value as SortMode)
                  }
                >
                  <option value="name">Name</option>
                  <option value="director">Director name</option>
                  <option value="played-count">Played count</option>
                  <option value="rating">Rating</option>
                  <option value="file-size">File size</option>
                  <option value="added">Added</option>
                </select>
              </label>
              <label htmlFor="sort-videos-secondary">
                Then sort
                <select
                  id="sort-videos-secondary"
                  value={secondarySortMode}
                  onChange={(event) =>
                    setSecondarySortMode(event.target.value as SecondarySortMode)
                  }
                >
                  <option value="none">None</option>
                  <option value="name">Name</option>
                  <option value="director">Director name</option>
                  <option value="played-count">Played count</option>
                  <option value="rating">Rating</option>
                  <option value="file-size">File size</option>
                  <option value="added">Added</option>
                </select>
              </label>
            </div>
            </div>
          )}

          <section
            className={
              [
                "content-layout",
                activeView === "settings" ? "settings-layout" : "",
                isRightPanelCollapsed ? "right-collapsed" : "",
              ]
                .filter(Boolean)
                .join(" ")
            }
            style={{ "--grid-card-min": `${gridSize}px` } as CSSProperties}
          >
            <div>{renderMainGrid()}</div>

            {activeView !== "settings" && (
              <>
                <button
                  aria-label={
                    isRightPanelCollapsed ? "Show right panel" : "Hide right panel"
                  }
                  className="panel-divider right-divider"
                  type="button"
                  onClick={() =>
                    setIsRightPanelCollapsed((collapsed) => !collapsed)
                  }
                />
                {!isRightPanelCollapsed && (
                  <aside className="details-panel">{renderRightPanel()}</aside>
                )}
              </>
            )}
          </section>
        </section>
      </section>

      {renderRefreshReportModal()}
      {renderStatisticsModal()}
      {renderBackupResultModal()}
      {renderRestorePreviewModal()}
      {renderRestoreResultModal()}
      {renderDeleteConfirmationModal()}
      {renderOrganizePreviewModal()}
      {renderOrganizeResultModal()}
      {renderOrganizeProgressPanel()}
      {renderFileOperationProgressModal("Creating Backup", backupProgress, isBackingUp)}
      {renderFileOperationProgressModal("Restoring Backup", restoreProgress, isRestoring)}
      {renderHoverPreview()}

      {actorCrop && (
        <div className="modal-backdrop">
          <section className="crop-dialog" aria-modal="true" role="dialog">
            <div className="crop-header">
              <h2>Actor Thumbnail</h2>
              <p>{actorCrop.actorName}</p>
            </div>

            <div className="crop-stage">
              <img
                alt=""
                className="crop-image"
                draggable={false}
                ref={actorCropImageRef}
                src={actorCrop.imageSrc}
                onLoad={prepareActorCrop}
              />
              <div
                className="crop-box"
                style={{
                  left: `${actorCrop.crop.x}%`,
                  top: `${actorCrop.crop.y}%`,
                  width: `${actorCrop.crop.width}%`,
                  height: `${actorCrop.crop.height}%`,
                }}
                onPointerDown={startActorCropDrag}
                onPointerMove={moveActorCrop}
                onPointerUp={stopActorCropDrag}
                onPointerCancel={stopActorCropDrag}
              />
            </div>

            <label className="crop-size-control" htmlFor="actor-crop-size">
              Crop size
              <input
                id="actor-crop-size"
                max="80"
                min="20"
                type="range"
                value={Math.round(actorCrop.crop.width)}
                onChange={(event) =>
                  updateActorCropSize(Number.parseInt(event.target.value, 10))
                }
              />
            </label>

            <div className="crop-actions">
              <button type="button" onClick={() => setActorCrop(null)}>
                Cancel
              </button>
              <button type="button" onClick={saveCroppedActorThumbnail}>
                Save Thumbnail
              </button>
            </div>
          </section>
        </div>
      )}

      {contextMenu && (
        <div
          className="context-menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onClick={(event) => event.stopPropagation()}
        >
          <button
            type="button"
            onClick={() => {
              openVideo(contextMenu.video.file_path);
              setContextMenu(null);
            }}
          >
            Open
          </button>
          <button
            type="button"
            onClick={() => {
              openVideoIncognito(contextMenu.video.file_path);
              setContextMenu(null);
            }}
          >
            Open Incognito
          </button>
          <button
            type="button"
            onClick={() => {
              openVideoDirectory(contextMenu.video.file_path);
              setContextMenu(null);
            }}
          >
            Open directory
          </button>
          <button
            type="button"
            onClick={() => {
              showActorVideos(contextMenu.video);
              setContextMenu(null);
            }}
          >
            Show actor&apos;s videos
          </button>
          <button
            type="button"
            onClick={() => {
              createThumbnailsForVideos(contextMenu.videos);
              setContextMenu(null);
            }}
          >
            {contextMenu.videos.length > 1
              ? `Create thumbnails (${contextMenu.videos.length})`
              : "Create thumbnail"}
          </button>
          <button
            type="button"
            onClick={() => {
              previewOrganizeVideos(contextMenu.videos);
              setContextMenu(null);
            }}
          >
            {contextMenu.videos.length > 1
              ? `Organize files (${contextMenu.videos.length})`
              : "Organize file"}
          </button>
          <button
            type="button"
            onClick={() => {
              backupSelectedVideos(contextMenu.videos);
              setContextMenu(null);
            }}
          >
            {contextMenu.videos.length > 1
              ? `Backup videos (${contextMenu.videos.length})`
              : "Backup video"}
          </button>
          <button
            type="button"
            onClick={() => {
              updateFileActorTags(contextMenu.videos);
              setContextMenu(null);
            }}
          >
            {contextMenu.videos.length > 1
              ? `Update meta tags of files (${contextMenu.videos.length})`
              : "Update meta tags of file"}
          </button>
          <button
            type="button"
            onClick={() => {
              requestDeleteSelectedVideoFiles(contextMenu.videos);
              setContextMenu(null);
            }}
          >
            {contextMenu.videos.length > 1
              ? `Delete videos (${contextMenu.videos.length})`
              : "Delete video"}
          </button>
        </div>
      )}
    </main>
  );
}

export default App;
