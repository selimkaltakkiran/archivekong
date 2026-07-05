use std::{
    collections::{hash_map::DefaultHasher, HashMap, HashSet},
    fs,
    fs::File,
    hash::{Hash, Hasher},
    io,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

#[derive(serde::Deserialize, serde::Serialize)]
struct VideoDatabase {
    #[serde(default)]
    folder_path: String,
    #[serde(default)]
    folder_paths: Vec<String>,
    videos: Vec<VideoFile>,
    #[serde(default)]
    actor_thumbnails: HashMap<String, String>,
    #[serde(default)]
    actor_bios: HashMap<String, String>,
    #[serde(default)]
    actor_social_links: HashMap<String, ActorSocialLinks>,
    #[serde(default)]
    watch_statistics: WatchStatistics,
}

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
struct WatchStatistics {
    #[serde(default)]
    total_watches: u64,
    #[serde(default)]
    actor_counts: HashMap<String, u64>,
    #[serde(default)]
    genre_counts: HashMap<String, u64>,
    #[serde(default)]
    year_counts: HashMap<String, u64>,
}

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
struct ActorSocialLinks {
    #[serde(default)]
    website: String,
    #[serde(default)]
    imdb: String,
    #[serde(default)]
    instagram: String,
    #[serde(default)]
    x: String,
    #[serde(default)]
    youtube: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct AppSettings {
    thumbnail_frame_second: String,
    grid_size: String,
    #[serde(default = "default_main_view_mode")]
    main_view_mode: String,
    #[serde(default = "default_font_size")]
    font_size: String,
    #[serde(default = "default_show_thumbnail_titles")]
    show_thumbnail_titles: bool,
    #[serde(default)]
    hide_explicit_content: bool,
    #[serde(default)]
    explicit_content_password_hash: String,
    #[serde(default = "default_sort_mode")]
    sort_mode: String,
    #[serde(default = "default_secondary_sort_mode")]
    secondary_sort_mode: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            thumbnail_frame_second: "1".to_string(),
            grid_size: "180".to_string(),
            main_view_mode: default_main_view_mode(),
            font_size: default_font_size(),
            show_thumbnail_titles: default_show_thumbnail_titles(),
            hide_explicit_content: false,
            explicit_content_password_hash: String::new(),
            sort_mode: default_sort_mode(),
            secondary_sort_mode: default_secondary_sort_mode(),
        }
    }
}

fn default_show_thumbnail_titles() -> bool {
    true
}

fn default_font_size() -> String {
    "large".to_string()
}

fn default_main_view_mode() -> String {
    "grid".to_string()
}

fn default_sort_mode() -> String {
    "played-count".to_string()
}

fn default_secondary_sort_mode() -> String {
    "rating".to_string()
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
struct VideoFile {
    filename: String,
    file_path: String,
    title: String,
    actor: String,
    genre: String,
    date: String,
    #[serde(default)]
    backup_date: String,
    #[serde(default)]
    backup_location: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    explicit_content: bool,
    #[serde(default)]
    resolution: String,
    #[serde(default)]
    bitrate: String,
    filesize: u64,
    artwork_thumbnail: String,
    #[serde(default)]
    rating: u64,
    #[serde(default)]
    play_count: u64,
    #[serde(default)]
    added_at: String,
}

#[derive(serde::Serialize)]
struct RefreshFieldChange {
    field: String,
    old_value: String,
    new_value: String,
}

#[derive(serde::Serialize)]
struct RefreshFileChange {
    file_path: String,
    filename: String,
    changes: Vec<RefreshFieldChange>,
}

#[derive(serde::Serialize)]
struct RefreshReport {
    database: VideoDatabase,
    changed_files: Vec<RefreshFileChange>,
    added_files: Vec<String>,
    removed_files: Vec<String>,
}

#[derive(serde::Serialize)]
struct OrganizePreview {
    base_folder: String,
    pattern: String,
    folders_to_create: Vec<String>,
    items: Vec<OrganizePreviewItem>,
}

#[derive(Clone, serde::Serialize)]
struct OrganizePreviewItem {
    file_path: String,
    filename: String,
    target_folder: String,
    target_path: String,
    renamed: bool,
    error: String,
}

#[derive(serde::Serialize)]
struct OrganizeResult {
    database: VideoDatabase,
    copied_count: usize,
    renamed_count: usize,
    updated_count: usize,
    errors: Vec<String>,
    items: Vec<OrganizePreviewItem>,
}

#[derive(Clone, serde::Serialize)]
struct OrganizeProgress {
    total: usize,
    completed: usize,
    current_file: String,
    status: String,
}

#[derive(Clone, serde::Serialize)]
struct FileOperationProgress {
    total: usize,
    completed: usize,
    current_file: String,
    status: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct BackupManifest {
    created_at: String,
    videos: Vec<BackupManifestVideo>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct BackupManifestVideo {
    filename: String,
    original_path: String,
    database_path: String,
    zip_path: String,
    title: String,
    actor: String,
    genre: String,
    date: String,
    filesize: u64,
}

#[derive(serde::Serialize)]
struct BackupResult {
    database: VideoDatabase,
    zip_path: String,
    backed_up_count: usize,
    errors: Vec<String>,
}

#[derive(serde::Serialize)]
struct RestorePreview {
    zip_path: String,
    items: Vec<RestorePreviewItem>,
}

#[derive(Clone, serde::Serialize)]
struct RestorePreviewItem {
    filename: String,
    target_path: String,
    exists: bool,
    error: String,
}

#[derive(serde::Serialize)]
struct RestoreResult {
    restored_count: usize,
    skipped_count: usize,
    errors: Vec<String>,
    items: Vec<RestorePreviewItem>,
}

#[derive(serde::Serialize)]
struct FileTagUpdateResult {
    updated_count: usize,
    skipped_count: usize,
    errors: Vec<String>,
}

#[derive(serde::Serialize)]
struct DeleteVideosResult {
    database: VideoDatabase,
    recycled_count: usize,
    removed_from_database_count: usize,
    errors: Vec<String>,
}

#[tauri::command]
fn load_video_database(app: tauri::AppHandle) -> Result<Option<VideoDatabase>, String> {
    let path = database_path(&app)?;

    if !path.exists() {
        return Ok(None);
    }

    let json =
        fs::read_to_string(&path).map_err(|error| format!("Could not read database: {error}"))?;
    let mut database: VideoDatabase = serde_json::from_str(&json)
        .map_err(|error| format!("Could not parse database JSON: {error}"))?;
    let changed = normalize_database(&mut database);
    if changed {
        save_database(&app, &database)?;
    }

    Ok(Some(database))
}

#[tauri::command]
fn scan_mp4_files(app: tauri::AppHandle, folder_path: String) -> Result<VideoDatabase, String> {
    let mut files = Vec::new();
    scan_folder(&app, &PathBuf::from(&folder_path), &mut files)?;

    files.sort_by(|first, second| first.title.cmp(&second.title));
    let database = VideoDatabase {
        folder_path: folder_path.clone(),
        folder_paths: vec![folder_path],
        videos: files,
        actor_thumbnails: HashMap::new(),
        actor_bios: HashMap::new(),
        actor_social_links: HashMap::new(),
        watch_statistics: WatchStatistics::default(),
    };
    save_database(&app, &database)?;

    Ok(database)
}

#[tauri::command]
fn add_video_directory(
    app: tauri::AppHandle,
    folder_path: String,
) -> Result<VideoDatabase, String> {
    let path = database_path(&app)?;
    let mut database = if path.exists() {
        let json = fs::read_to_string(&path)
            .map_err(|error| format!("Could not read database: {error}"))?;
        serde_json::from_str(&json)
            .map_err(|error| format!("Could not parse database JSON: {error}"))?
    } else {
        VideoDatabase {
            folder_path: folder_path.clone(),
            folder_paths: Vec::new(),
            videos: Vec::new(),
            actor_thumbnails: HashMap::new(),
            actor_bios: HashMap::new(),
            actor_social_links: HashMap::new(),
            watch_statistics: WatchStatistics::default(),
        }
    };
    normalize_database(&mut database);

    if !database
        .folder_paths
        .iter()
        .any(|path| path == &folder_path)
    {
        database.folder_paths.push(folder_path.clone());
    }

    if database.folder_path.is_empty() {
        database.folder_path = folder_path.clone();
    }

    let mut new_files = Vec::new();
    scan_folder(&app, &PathBuf::from(&folder_path), &mut new_files)?;

    let existing_paths: HashSet<String> = database
        .videos
        .iter()
        .map(|video| video.file_path.clone())
        .collect();

    database.videos.extend(
        new_files
            .into_iter()
            .filter(|video| !existing_paths.contains(&video.file_path)),
    );
    database
        .videos
        .sort_by(|first, second| first.title.cmp(&second.title));
    save_database(&app, &database)?;

    Ok(database)
}

#[tauri::command]
fn refresh_video_database(app: tauri::AppHandle) -> Result<RefreshReport, String> {
    let path = database_path(&app)?;

    if !path.exists() {
        return Err("Database does not exist yet.".to_string());
    }

    let json =
        fs::read_to_string(&path).map_err(|error| format!("Could not read database: {error}"))?;
    let mut database: VideoDatabase = serde_json::from_str(&json)
        .map_err(|error| format!("Could not parse database JSON: {error}"))?;
    normalize_database(&mut database);

    let mut scanned_files = Vec::new();
    for folder_path in database.folder_paths.clone() {
        scan_folder(&app, &PathBuf::from(folder_path), &mut scanned_files)?;
    }

    let mut existing_by_path: HashMap<String, VideoFile> = database
        .videos
        .into_iter()
        .map(|video| (video.file_path.clone(), video))
        .collect();
    let mut seen_scanned_paths = HashSet::new();
    let mut refreshed_videos = Vec::new();
    let mut changed_files = Vec::new();
    let mut added_files = Vec::new();

    for scanned_video in scanned_files {
        if !seen_scanned_paths.insert(scanned_video.file_path.clone()) {
            continue;
        }

        if let Some(mut existing_video) = existing_by_path.remove(&scanned_video.file_path) {
            let changes = refresh_video_fields(&mut existing_video, &scanned_video);

            if !changes.is_empty() {
                changed_files.push(RefreshFileChange {
                    file_path: existing_video.file_path.clone(),
                    filename: existing_video.filename.clone(),
                    changes,
                });
            }

            refreshed_videos.push(existing_video);
        } else {
            added_files.push(scanned_video.file_path.clone());
            refreshed_videos.push(scanned_video);
        }
    }

    let removed_files = existing_by_path
        .into_values()
        .map(|video| video.file_path)
        .collect::<Vec<_>>();

    refreshed_videos.sort_by(|first, second| first.title.cmp(&second.title));
    database.videos = refreshed_videos;
    save_database(&app, &database)?;

    Ok(RefreshReport {
        database,
        changed_files,
        added_files,
        removed_files,
    })
}

fn refresh_video_fields(
    existing_video: &mut VideoFile,
    scanned_video: &VideoFile,
) -> Vec<RefreshFieldChange> {
    let mut changes = Vec::new();

    refresh_string_field(
        "filename",
        &mut existing_video.filename,
        &scanned_video.filename,
        &mut changes,
    );
    refresh_string_field(
        "title",
        &mut existing_video.title,
        &scanned_video.title,
        &mut changes,
    );
    refresh_string_field(
        "actor",
        &mut existing_video.actor,
        &scanned_video.actor,
        &mut changes,
    );
    refresh_string_field(
        "genre",
        &mut existing_video.genre,
        &scanned_video.genre,
        &mut changes,
    );
    refresh_string_field(
        "date",
        &mut existing_video.date,
        &scanned_video.date,
        &mut changes,
    );
    refresh_u64_field(
        "filesize",
        &mut existing_video.filesize,
        scanned_video.filesize,
        &mut changes,
    );
    refresh_u64_field(
        "rating",
        &mut existing_video.rating,
        scanned_video.rating,
        &mut changes,
    );

    changes
}

fn refresh_string_field(
    field: &str,
    current_value: &mut String,
    new_value: &str,
    changes: &mut Vec<RefreshFieldChange>,
) {
    if current_value == new_value {
        return;
    }

    changes.push(RefreshFieldChange {
        field: field.to_string(),
        old_value: current_value.clone(),
        new_value: new_value.to_string(),
    });
    *current_value = new_value.to_string();
}

fn refresh_u64_field(
    field: &str,
    current_value: &mut u64,
    new_value: u64,
    changes: &mut Vec<RefreshFieldChange>,
) {
    if *current_value == new_value {
        return;
    }

    changes.push(RefreshFieldChange {
        field: field.to_string(),
        old_value: current_value.to_string(),
        new_value: new_value.to_string(),
    });
    *current_value = new_value;
}

fn scan_folder(
    app: &tauri::AppHandle,
    folder_path: &PathBuf,
    files: &mut Vec<VideoFile>,
) -> Result<(), String> {
    let entries =
        fs::read_dir(folder_path).map_err(|error| format!("Could not read folder: {error}"))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("Could not read folder entry: {error}"))?;
        let path = entry.path();

        if path.is_dir() {
            scan_folder(app, &path, files)?;
            continue;
        }

        if !path.is_file() {
            continue;
        }

        let is_supported_video = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(is_supported_video_extension);

        if is_supported_video {
            let filename = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();

            let metadata = fs::metadata(&path)
                .map_err(|error| format!("Could not read file metadata: {error}"))?;
            let tag = mp4ameta::Tag::read_from_path(&path).ok();
            let title = tag
                .as_ref()
                .and_then(|tag| tag.title().map(str::to_string))
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| filename.clone());
            let actor = tag
                .as_ref()
                .and_then(|tag| tag.artist().map(str::to_string))
                .unwrap_or_default();
            let genre = tag
                .as_ref()
                .and_then(|tag| tag.genre().map(str::to_string))
                .unwrap_or_default();
            let date = tag
                .as_ref()
                .and_then(|tag| tag.year().map(str::to_string))
                .unwrap_or_default();
            let rating = tag.as_ref().map(read_rating_from_tag).unwrap_or(0);
            let file_path = path.to_string_lossy().to_string();
            let cached_thumbnail = thumbnail_path(&app, &file_path)?;
            let artwork_thumbnail = if cached_thumbnail.exists() {
                cached_thumbnail.to_string_lossy().to_string()
            } else {
                file_path.clone()
            };

            files.push(VideoFile {
                filename,
                file_path: file_path.clone(),
                title,
                actor,
                genre,
                date,
                backup_date: String::new(),
                backup_location: String::new(),
                notes: String::new(),
                explicit_content: false,
                resolution: String::new(),
                bitrate: String::new(),
                filesize: metadata.len(),
                artwork_thumbnail,
                rating,
                play_count: 0,
                added_at: current_timestamp(),
            });
        }
    }

    Ok(())
}

fn is_supported_video_extension(extension: &str) -> bool {
    matches!(
        extension.to_lowercase().as_str(),
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" | "m4v"
    )
}

fn is_supported_mp4_tag_extension(file_path: &str) -> bool {
    PathBuf::from(file_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_lowercase().as_str(), "mp4" | "m4v" | "mov")
        })
}

#[tauri::command]
fn open_video_file(app: tauri::AppHandle, file_path: String) -> Result<u64, String> {
    app.opener()
        .open_path(&file_path, None::<&str>)
        .map_err(|error| format!("Could not open video file: {error}"))?;

    increment_play_count(&app, &file_path)
}

#[tauri::command]
fn open_video_file_incognito(app: tauri::AppHandle, file_path: String) -> Result<(), String> {
    app.opener()
        .open_path(&file_path, None::<&str>)
        .map_err(|error| format!("Could not open video file: {error}"))
}

#[tauri::command]
fn open_video_directory(app: tauri::AppHandle, file_path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let _ = &app;

    let path = PathBuf::from(file_path);

    #[cfg(target_os = "windows")]
    {
        if !path.exists() {
            return Err("Video file does not exist.".to_string());
        }

        let selection_arg = format!("/select,\"{}\"", path.to_string_lossy());

        Command::new("explorer.exe")
            .raw_arg(selection_arg)
            .spawn()
            .map_err(|error| format!("Could not open video directory: {error}"))?;

        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let Some(parent_path) = path.parent() else {
            return Err("Could not find video directory.".to_string());
        };

        app.opener()
            .open_path(parent_path.to_string_lossy().to_string(), None::<&str>)
            .map_err(|error| format!("Could not open video directory: {error}"))
    }
}

#[tauri::command]
fn update_video_file_actor_tags(
    app: tauri::AppHandle,
    file_paths: Vec<String>,
) -> Result<FileTagUpdateResult, String> {
    let database = load_required_database(&app)?;
    let requested_paths = file_paths.into_iter().collect::<HashSet<_>>();
    let mut updated_count = 0;
    let mut skipped_count = 0;
    let mut errors = Vec::new();

    for video in database
        .videos
        .iter()
        .filter(|video| requested_paths.contains(&video.file_path))
    {
        if !is_supported_mp4_tag_extension(&video.file_path) {
            skipped_count += 1;
            errors.push(format!(
                "{}: File type is not supported for MP4 metadata writing.",
                video.filename
            ));
            continue;
        }

        let path = PathBuf::from(&video.file_path);
        if !path.exists() {
            skipped_count += 1;
            errors.push(format!("{}: File does not exist.", video.filename));
            continue;
        }

        match mp4ameta::Tag::read_from_path(&path) {
            Ok(mut tag) => {
                let title = video.title.trim();
                if title.is_empty() {
                    tag.remove_title();
                } else {
                    tag.set_title(title.to_string());
                }

                let actor = video.actor.trim();
                if actor.is_empty() {
                    tag.remove_artists();
                } else {
                    tag.set_artist(actor.to_string());
                }

                let genre = video.genre.trim();
                if genre.is_empty() {
                    tag.remove_genres();
                } else {
                    tag.set_genre(genre.to_string());
                }

                let date = video.date.trim();
                if date.is_empty() {
                    tag.remove_year();
                } else {
                    tag.set_year(date.to_string());
                }

                let notes = video.notes.trim();
                if notes.is_empty() {
                    tag.remove_comments();
                } else {
                    tag.set_comment(notes.to_string());
                }

                set_video_rating_tag(&mut tag, video.rating.min(10));

                match tag.write_to_path(&path) {
                    Ok(()) => updated_count += 1,
                    Err(error) => {
                        skipped_count += 1;
                        errors.push(format!(
                            "{}: Could not write metadata: {error}",
                            video.filename
                        ));
                    }
                }
            }
            Err(error) => {
                skipped_count += 1;
                errors.push(format!(
                    "{}: Could not read MP4 metadata: {error}",
                    video.filename
                ));
            }
        }
    }

    Ok(FileTagUpdateResult {
        updated_count,
        skipped_count,
        errors,
    })
}

#[tauri::command]
fn delete_videos(
    app: tauri::AppHandle,
    file_paths: Vec<String>,
) -> Result<DeleteVideosResult, String> {
    let mut database = load_required_database(&app)?;
    let requested_paths = file_paths.into_iter().collect::<HashSet<_>>();
    let mut deleted_paths = HashSet::new();
    let mut recycled_count = 0;
    let mut errors = Vec::new();

    for video in database
        .videos
        .iter()
        .filter(|video| requested_paths.contains(&video.file_path))
    {
        let path = PathBuf::from(&video.file_path);

        if !path.exists() {
            deleted_paths.insert(video.file_path.clone());
            continue;
        }

        if !path.is_file() {
            errors.push(format!("{}: Path is not a file.", video.filename));
            continue;
        }

        match trash::delete(&path) {
            Ok(()) => {
                recycled_count += 1;
                deleted_paths.insert(video.file_path.clone());
            }
            Err(error) => {
                errors.push(format!(
                    "{}: Could not move file to Recycle Bin: {error}",
                    video.filename
                ));
            }
        }
    }

    let original_count = database.videos.len();
    database
        .videos
        .retain(|video| !deleted_paths.contains(&video.file_path));
    let removed_from_database_count = original_count - database.videos.len();

    if removed_from_database_count > 0 {
        save_database(&app, &database)?;
    }

    Ok(DeleteVideosResult {
        database,
        recycled_count,
        removed_from_database_count,
        errors,
    })
}

#[tauri::command]
fn preview_organize_videos(
    app: tauri::AppHandle,
    file_paths: Vec<String>,
    base_folder: String,
    pattern: String,
) -> Result<OrganizePreview, String> {
    let database = load_required_database(&app)?;
    let requested_paths = file_paths.into_iter().collect::<HashSet<_>>();
    let requested_videos = database
        .videos
        .iter()
        .filter(|video| requested_paths.contains(&video.file_path))
        .cloned()
        .collect::<Vec<_>>();

    Ok(build_organize_preview(
        &requested_videos,
        &base_folder,
        &pattern,
    ))
}

#[tauri::command]
fn confirm_organize_videos(
    app: tauri::AppHandle,
    file_paths: Vec<String>,
    base_folder: String,
    pattern: String,
) -> Result<OrganizeResult, String> {
    let mut database = load_required_database(&app)?;
    let requested_paths = file_paths.into_iter().collect::<HashSet<_>>();
    let requested_videos = database
        .videos
        .iter()
        .filter(|video| requested_paths.contains(&video.file_path))
        .cloned()
        .collect::<Vec<_>>();
    let preview = build_organize_preview(&requested_videos, &base_folder, &pattern);
    let mut copied_count = 0;
    let mut renamed_count = 0;
    let mut updated_count = 0;
    let mut completed_count = 0;
    let mut errors = Vec::new();
    let total_count = preview.items.len();

    emit_organize_progress(
        &app,
        total_count,
        completed_count,
        "Preparing files".to_string(),
        "copying",
    );

    for item in &preview.items {
        emit_organize_progress(
            &app,
            total_count,
            completed_count,
            item.filename.clone(),
            "copying",
        );

        if !item.error.is_empty() {
            errors.push(format!("{}: {}", item.filename, item.error));
            completed_count += 1;
            emit_organize_progress(
                &app,
                total_count,
                completed_count,
                item.filename.clone(),
                "skipped",
            );
            continue;
        }

        if let Err(error) = fs::create_dir_all(&item.target_folder) {
            errors.push(format!(
                "{}: Could not create target folder: {error}",
                item.filename
            ));
            completed_count += 1;
            emit_organize_progress(
                &app,
                total_count,
                completed_count,
                item.filename.clone(),
                "error",
            );
            continue;
        }

        if let Err(error) = fs::copy(&item.file_path, &item.target_path) {
            errors.push(format!("{}: Could not copy file: {error}", item.filename));
            completed_count += 1;
            emit_organize_progress(
                &app,
                total_count,
                completed_count,
                item.filename.clone(),
                "error",
            );
            continue;
        }

        copied_count += 1;
        completed_count += 1;
        emit_organize_progress(
            &app,
            total_count,
            completed_count,
            item.filename.clone(),
            "copied",
        );

        if item.renamed {
            renamed_count += 1;
        }

        if let Some(video) = database
            .videos
            .iter_mut()
            .find(|video| video.file_path == item.file_path)
        {
            video.file_path = item.target_path.clone();
            video.filename = PathBuf::from(&item.target_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&item.filename)
                .to_string();
            updated_count += 1;
        }
    }

    if !database
        .folder_paths
        .iter()
        .any(|folder_path| folder_path == &base_folder)
    {
        database.folder_paths.push(base_folder.clone());
    }

    if database.folder_path.is_empty() {
        database.folder_path = base_folder.clone();
    }

    save_database(&app, &database)?;
    emit_organize_progress(
        &app,
        total_count,
        completed_count,
        "Finished".to_string(),
        "done",
    );

    Ok(OrganizeResult {
        database,
        copied_count,
        renamed_count,
        updated_count,
        errors,
        items: preview.items,
    })
}

fn emit_organize_progress(
    app: &tauri::AppHandle,
    total: usize,
    completed: usize,
    current_file: String,
    status: &str,
) {
    let _ = app.emit(
        "organize-progress",
        OrganizeProgress {
            total,
            completed,
            current_file,
            status: status.to_string(),
        },
    );
}

#[tauri::command]
fn backup_videos(
    app: tauri::AppHandle,
    file_paths: Vec<String>,
    destination_folder: String,
) -> Result<BackupResult, String> {
    let mut database = load_required_database(&app)?;
    let requested_paths = file_paths.into_iter().collect::<HashSet<_>>();
    let selected_videos = database
        .videos
        .iter()
        .filter(|video| requested_paths.contains(&video.file_path))
        .cloned()
        .collect::<Vec<_>>();

    if selected_videos.is_empty() {
        return Err("No selected videos were found in database.".to_string());
    }

    fs::create_dir_all(&destination_folder)
        .map_err(|error| format!("Could not create backup folder: {error}"))?;

    let created_at = current_timestamp();
    let zip_path =
        PathBuf::from(&destination_folder).join(format!("video-library-backup-{created_at}.zip"));
    let zip_file =
        File::create(&zip_path).map_err(|error| format!("Could not create zip file: {error}"))?;
    let mut zip = zip::ZipWriter::new(zip_file);
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut manifest = BackupManifest {
        created_at: created_at.clone(),
        videos: Vec::new(),
    };
    let mut errors = Vec::new();
    let total_count = selected_videos.len();
    let mut completed_count = 0;

    emit_file_operation_progress(
        &app,
        "backup-progress",
        total_count,
        completed_count,
        "Preparing backup".to_string(),
        "copying",
    );

    for video in &selected_videos {
        emit_file_operation_progress(
            &app,
            "backup-progress",
            total_count,
            completed_count,
            video.filename.clone(),
            "copying",
        );

        let source_path = PathBuf::from(&video.file_path);
        if !source_path.exists() {
            errors.push(format!("{}: Source file does not exist.", video.filename));
            completed_count += 1;
            continue;
        }

        let internal_path = format!(
            "videos/{}-{}",
            hashed_path_name(&video.file_path),
            sanitize_zip_filename(&video.filename)
        );

        if let Err(error) = zip.start_file(&internal_path, options) {
            errors.push(format!(
                "{}: Could not add file to zip: {error}",
                video.filename
            ));
            completed_count += 1;
            continue;
        }

        match File::open(&source_path) {
            Ok(mut source_file) => {
                if let Err(error) = io::copy(&mut source_file, &mut zip) {
                    errors.push(format!(
                        "{}: Could not write video to zip: {error}",
                        video.filename
                    ));
                    completed_count += 1;
                    continue;
                }
            }
            Err(error) => {
                errors.push(format!(
                    "{}: Could not read video file: {error}",
                    video.filename
                ));
                completed_count += 1;
                continue;
            }
        }

        manifest.videos.push(BackupManifestVideo {
            filename: video.filename.clone(),
            original_path: video.file_path.clone(),
            database_path: video.file_path.clone(),
            zip_path: internal_path,
            title: video.title.clone(),
            actor: video.actor.clone(),
            genre: video.genre.clone(),
            date: video.date.clone(),
            filesize: video.filesize,
        });

        completed_count += 1;
        emit_file_operation_progress(
            &app,
            "backup-progress",
            total_count,
            completed_count,
            video.filename.clone(),
            "copied",
        );
    }

    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("Could not create backup manifest: {error}"))?;
    zip.start_file("backup-manifest.json", options)
        .map_err(|error| format!("Could not add manifest to zip: {error}"))?;
    io::copy(&mut manifest_json.as_slice(), &mut zip)
        .map_err(|error| format!("Could not write manifest to zip: {error}"))?;

    let database_json = serde_json::to_vec_pretty(&database)
        .map_err(|error| format!("Could not create database snapshot: {error}"))?;
    zip.start_file("video-database-snapshot.json", options)
        .map_err(|error| format!("Could not add database snapshot to zip: {error}"))?;
    io::copy(&mut database_json.as_slice(), &mut zip)
        .map_err(|error| format!("Could not write database snapshot to zip: {error}"))?;
    zip.finish()
        .map_err(|error| format!("Could not finish zip file: {error}"))?;

    let zip_path_string = zip_path.to_string_lossy().to_string();
    let backed_up_paths = manifest
        .videos
        .iter()
        .map(|video| video.database_path.clone())
        .collect::<HashSet<_>>();

    for video in &mut database.videos {
        if backed_up_paths.contains(&video.file_path) {
            video.backup_date = created_at.clone();
            video.backup_location = zip_path_string.clone();
        }
    }

    save_database(&app, &database)?;
    emit_file_operation_progress(
        &app,
        "backup-progress",
        total_count,
        completed_count,
        "Finished".to_string(),
        "done",
    );

    Ok(BackupResult {
        database,
        zip_path: zip_path_string,
        backed_up_count: manifest.videos.len(),
        errors,
    })
}

#[tauri::command]
fn preview_restore_backup(zip_path: String) -> Result<RestorePreview, String> {
    let manifest = read_backup_manifest(&zip_path)?;
    let items = manifest
        .videos
        .into_iter()
        .map(|video| {
            let target_path = PathBuf::from(&video.database_path);
            let exists = target_path.exists();
            let error = if video.zip_path.trim().is_empty() {
                "Backup entry is missing zip path.".to_string()
            } else {
                String::new()
            };

            RestorePreviewItem {
                filename: video.filename,
                target_path: video.database_path,
                exists,
                error,
            }
        })
        .collect();

    Ok(RestorePreview { zip_path, items })
}

#[tauri::command]
fn restore_backup(app: tauri::AppHandle, zip_path: String) -> Result<RestoreResult, String> {
    let manifest = read_backup_manifest(&zip_path)?;
    let zip_file =
        File::open(&zip_path).map_err(|error| format!("Could not open backup zip: {error}"))?;
    let mut archive = zip::ZipArchive::new(zip_file)
        .map_err(|error| format!("Could not read backup zip: {error}"))?;
    let total_count = manifest.videos.len();
    let mut completed_count = 0;
    let mut restored_count = 0;
    let mut skipped_count = 0;
    let mut errors = Vec::new();
    let mut items = Vec::new();

    emit_file_operation_progress(
        &app,
        "restore-progress",
        total_count,
        completed_count,
        "Preparing restore".to_string(),
        "copying",
    );

    for video in manifest.videos {
        emit_file_operation_progress(
            &app,
            "restore-progress",
            total_count,
            completed_count,
            video.filename.clone(),
            "copying",
        );

        let target_path = PathBuf::from(&video.database_path);
        let exists = target_path.exists();
        let mut item = RestorePreviewItem {
            filename: video.filename.clone(),
            target_path: video.database_path.clone(),
            exists,
            error: String::new(),
        };

        if exists {
            skipped_count += 1;
            completed_count += 1;
            items.push(item);
            continue;
        }

        if let Some(parent) = target_path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                item.error = format!("Could not create target folder: {error}");
                errors.push(format!("{}: {}", video.filename, item.error));
                completed_count += 1;
                items.push(item);
                continue;
            }
        }

        let mut backup_file = match archive.by_name(&video.zip_path) {
            Ok(file) => file,
            Err(error) => {
                item.error = format!("Could not find file in zip: {error}");
                errors.push(format!("{}: {}", video.filename, item.error));
                completed_count += 1;
                items.push(item);
                continue;
            }
        };

        match File::create(&target_path) {
            Ok(mut output_file) => {
                if let Err(error) = io::copy(&mut backup_file, &mut output_file) {
                    item.error = format!("Could not restore file: {error}");
                    errors.push(format!("{}: {}", video.filename, item.error));
                } else {
                    restored_count += 1;
                }
            }
            Err(error) => {
                item.error = format!("Could not create target file: {error}");
                errors.push(format!("{}: {}", video.filename, item.error));
            }
        }

        completed_count += 1;
        emit_file_operation_progress(
            &app,
            "restore-progress",
            total_count,
            completed_count,
            video.filename.clone(),
            if item.error.is_empty() {
                "copied"
            } else {
                "error"
            },
        );
        items.push(item);
    }

    emit_file_operation_progress(
        &app,
        "restore-progress",
        total_count,
        completed_count,
        "Finished".to_string(),
        "done",
    );

    Ok(RestoreResult {
        restored_count,
        skipped_count,
        errors,
        items,
    })
}

fn read_backup_manifest(zip_path: &str) -> Result<BackupManifest, String> {
    let zip_file =
        File::open(zip_path).map_err(|error| format!("Could not open backup zip: {error}"))?;
    let mut archive = zip::ZipArchive::new(zip_file)
        .map_err(|error| format!("Could not read backup zip: {error}"))?;
    let manifest_file = archive
        .by_name("backup-manifest.json")
        .map_err(|error| format!("Could not find backup manifest: {error}"))?;

    serde_json::from_reader(manifest_file)
        .map_err(|error| format!("Could not parse backup manifest: {error}"))
}

fn emit_file_operation_progress(
    app: &tauri::AppHandle,
    event_name: &str,
    total: usize,
    completed: usize,
    current_file: String,
    status: &str,
) {
    let _ = app.emit(
        event_name,
        FileOperationProgress {
            total,
            completed,
            current_file,
            status: status.to_string(),
        },
    );
}

fn hashed_path_name(path: &str) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn sanitize_zip_filename(filename: &str) -> String {
    filename
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            _ => character,
        })
        .collect()
}

fn load_required_database(app: &tauri::AppHandle) -> Result<VideoDatabase, String> {
    let path = database_path(app)?;

    if !path.exists() {
        return Err("Database does not exist yet.".to_string());
    }

    let json =
        fs::read_to_string(&path).map_err(|error| format!("Could not read database: {error}"))?;
    let mut database: VideoDatabase = serde_json::from_str(&json)
        .map_err(|error| format!("Could not parse database JSON: {error}"))?;
    normalize_database(&mut database);

    Ok(database)
}

fn build_organize_preview(
    videos: &[VideoFile],
    base_folder: &str,
    pattern: &str,
) -> OrganizePreview {
    let base_folder_path = PathBuf::from(base_folder);
    let mut planned_targets = HashSet::<PathBuf>::new();
    let mut folders_to_create = HashSet::<String>::new();
    let mut items = Vec::new();

    for video in videos {
        let target_folder = base_folder_path.join(resolve_organize_pattern(pattern, video));
        let mut target_path = target_folder.join(&video.filename);
        let mut renamed = false;
        let mut error = String::new();

        if !PathBuf::from(&video.file_path).exists() {
            error = "Source file does not exist.".to_string();
        }

        if target_path.exists() || planned_targets.contains(&target_path) {
            renamed = true;
            target_path = unique_target_path(&target_folder, &video.filename, &planned_targets);
        }

        planned_targets.insert(target_path.clone());

        if !target_folder.exists() {
            folders_to_create.insert(target_folder.to_string_lossy().to_string());
        }

        items.push(OrganizePreviewItem {
            file_path: video.file_path.clone(),
            filename: video.filename.clone(),
            target_folder: target_folder.to_string_lossy().to_string(),
            target_path: target_path.to_string_lossy().to_string(),
            renamed,
            error,
        });
    }

    let mut folders_to_create = folders_to_create.into_iter().collect::<Vec<_>>();
    folders_to_create.sort();

    OrganizePreview {
        base_folder: base_folder.to_string(),
        pattern: pattern.to_string(),
        folders_to_create,
        items,
    }
}

fn resolve_organize_pattern(pattern: &str, video: &VideoFile) -> PathBuf {
    let year = first_year(&video.date).unwrap_or_else(|| "Unknown Year".to_string());
    let rating = if video.rating == 0 {
        "Unrated".to_string()
    } else {
        video.rating.to_string()
    };
    let resolved = pattern
        .replace(
            "{actor}",
            &sanitize_path_part(first_tag_value(&video.actor, "Unknown Actor")),
        )
        .replace(
            "{genre}",
            &sanitize_path_part(first_tag_value(&video.genre, "Unknown Genre")),
        )
        .replace(
            "{date}",
            &sanitize_path_part(default_if_empty(&video.date, "Unknown Date")),
        )
        .replace("{year}", &sanitize_path_part(&year))
        .replace("{rating}", &sanitize_path_part(&rating));

    resolved
        .split(['\\', '/'])
        .filter(|part| !part.trim().is_empty())
        .fold(PathBuf::new(), |path, part| path.join(part))
}

fn first_tag_value<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    value
        .split([',', ';', '|', '/'])
        .map(str::trim)
        .find(|part| !part.is_empty())
        .unwrap_or(fallback)
}

fn default_if_empty<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value.trim()
    }
}

fn first_year(value: &str) -> Option<String> {
    let mut buffer = String::new();

    for character in value.chars() {
        if character.is_ascii_digit() {
            buffer.push(character);
            if buffer.len() == 4 {
                return Some(buffer);
            }
        } else {
            buffer.clear();
        }
    }

    None
}

fn sanitize_path_part(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            _ => character,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();

    if sanitized.is_empty() {
        "Unknown".to_string()
    } else {
        sanitized
    }
}

fn unique_target_path(
    target_folder: &PathBuf,
    filename: &str,
    planned_targets: &HashSet<PathBuf>,
) -> PathBuf {
    let original_path = PathBuf::from(filename);
    let stem = original_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("video");
    let extension = original_path
        .extension()
        .and_then(|extension| extension.to_str());
    let mut index = 2;

    loop {
        let candidate_filename = if let Some(extension) = extension {
            format!("{stem} ({index}).{extension}")
        } else {
            format!("{stem} ({index})")
        };
        let candidate_path = target_folder.join(candidate_filename);

        if !candidate_path.exists() && !planned_targets.contains(&candidate_path) {
            return candidate_path;
        }

        index += 1;
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            load_video_database,
            scan_mp4_files,
            add_video_directory,
            refresh_video_database,
            reset_watch_statistics,
            reset_video_database,
            database_file_path,
            open_database_file,
            export_database_file,
            read_image_file,
            load_app_settings,
            save_app_settings,
            update_actor_thumbnail,
            save_actor_thumbnail,
            update_actor_bio,
            update_actor_social_links,
            save_video_thumbnail,
            update_video_metadata,
            update_multiple_video_metadata,
            update_video_technical_metadata,
            open_video_file,
            open_video_file_incognito,
            open_video_directory,
            update_video_file_actor_tags,
            delete_videos,
            backup_videos,
            preview_restore_backup,
            restore_backup,
            preview_organize_videos,
            confirm_organize_videos
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn database_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not find app data folder: {error}"))?
        .join("video-database.json"))
}

fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not find app data folder: {error}"))?
        .join("app-settings.json"))
}

#[tauri::command]
fn database_file_path(app: tauri::AppHandle) -> Result<String, String> {
    Ok(database_path(&app)?.to_string_lossy().to_string())
}

#[tauri::command]
fn load_app_settings(app: tauri::AppHandle) -> Result<AppSettings, String> {
    let path = settings_path(&app)?;

    if !path.exists() {
        return Ok(AppSettings::default());
    }

    let json =
        fs::read_to_string(&path).map_err(|error| format!("Could not read settings: {error}"))?;
    let settings = serde_json::from_str(&json)
        .map_err(|error| format!("Could not parse settings: {error}"))?;

    Ok(settings)
}

#[tauri::command]
fn save_app_settings(
    app: tauri::AppHandle,
    thumbnail_frame_second: String,
    grid_size: String,
    main_view_mode: String,
    font_size: String,
    show_thumbnail_titles: bool,
    hide_explicit_content: bool,
    explicit_content_password_hash: String,
    sort_mode: String,
    secondary_sort_mode: String,
) -> Result<(), String> {
    let settings = AppSettings {
        thumbnail_frame_second,
        grid_size,
        main_view_mode,
        font_size,
        show_thumbnail_titles,
        hide_explicit_content,
        explicit_content_password_hash,
        sort_mode,
        secondary_sort_mode,
    };
    let path = settings_path(&app)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create settings folder: {error}"))?;
    }

    let json = serde_json::to_string_pretty(&settings)
        .map_err(|error| format!("Could not create settings JSON: {error}"))?;

    fs::write(&path, json).map_err(|error| format!("Could not save settings: {error}"))
}

#[tauri::command]
fn open_database_file(app: tauri::AppHandle) -> Result<(), String> {
    let path = database_path(&app)?;

    if !path.exists() {
        return Err("Database does not exist yet.".to_string());
    }

    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| format!("Could not open database JSON: {error}"))
}

#[tauri::command]
fn export_database_file(app: tauri::AppHandle, folder_path: String) -> Result<String, String> {
    let source_path = database_path(&app)?;

    if !source_path.exists() {
        return Err("Database does not exist yet.".to_string());
    }

    let target_path = PathBuf::from(folder_path).join("video-database.json");
    fs::copy(&source_path, &target_path)
        .map_err(|error| format!("Could not export database JSON: {error}"))?;

    Ok(target_path.to_string_lossy().to_string())
}

#[tauri::command]
fn read_image_file(file_path: String) -> Result<Vec<u8>, String> {
    fs::read(file_path).map_err(|error| format!("Could not read image file: {error}"))
}

#[tauri::command]
fn update_actor_thumbnail(
    app: tauri::AppHandle,
    actor_name: String,
    thumbnail_path: String,
) -> Result<HashMap<String, String>, String> {
    let path = database_path(&app)?;

    if !path.exists() {
        return Err("Database does not exist.".to_string());
    }

    let json =
        fs::read_to_string(&path).map_err(|error| format!("Could not read database: {error}"))?;
    let mut database: VideoDatabase = serde_json::from_str(&json)
        .map_err(|error| format!("Could not parse database JSON: {error}"))?;

    database.actor_thumbnails.insert(actor_name, thumbnail_path);
    save_database(&app, &database)?;

    Ok(database.actor_thumbnails)
}

#[tauri::command]
fn save_actor_thumbnail(
    app: tauri::AppHandle,
    actor_name: String,
    image_bytes: Vec<u8>,
) -> Result<HashMap<String, String>, String> {
    let mut hasher = DefaultHasher::new();
    actor_name.hash(&mut hasher);

    let thumbnail_path = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not find app data folder: {error}"))?
        .join("actor-thumbnails")
        .join(format!("{:x}.jpg", hasher.finish()));

    if let Some(parent) = thumbnail_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create actor thumbnail folder: {error}"))?;
    }

    fs::write(&thumbnail_path, image_bytes)
        .map_err(|error| format!("Could not save actor thumbnail: {error}"))?;

    update_actor_thumbnail(
        app,
        actor_name,
        thumbnail_path.to_string_lossy().to_string(),
    )
}

#[tauri::command]
fn update_actor_bio(
    app: tauri::AppHandle,
    actor_name: String,
    bio: String,
) -> Result<HashMap<String, String>, String> {
    let path = database_path(&app)?;

    if !path.exists() {
        return Err("Database does not exist.".to_string());
    }

    let json =
        fs::read_to_string(&path).map_err(|error| format!("Could not read database: {error}"))?;
    let mut database: VideoDatabase = serde_json::from_str(&json)
        .map_err(|error| format!("Could not parse database JSON: {error}"))?;

    database.actor_bios.insert(actor_name, bio);
    save_database(&app, &database)?;

    Ok(database.actor_bios)
}

#[tauri::command]
fn update_actor_social_links(
    app: tauri::AppHandle,
    actor_name: String,
    social_links: ActorSocialLinks,
) -> Result<HashMap<String, ActorSocialLinks>, String> {
    let path = database_path(&app)?;

    if !path.exists() {
        return Err("Database does not exist.".to_string());
    }

    let json =
        fs::read_to_string(&path).map_err(|error| format!("Could not read database: {error}"))?;
    let mut database: VideoDatabase = serde_json::from_str(&json)
        .map_err(|error| format!("Could not parse database JSON: {error}"))?;

    database.actor_social_links.insert(actor_name, social_links);
    save_database(&app, &database)?;

    Ok(database.actor_social_links)
}

fn read_rating_from_tag(tag: &mp4ameta::Tag) -> u64 {
    for (ident, value) in tag.strings() {
        if is_rating_ident(ident) {
            if let Some(rating) = parse_rating_text(value) {
                return rating;
            }
        }
    }

    for (ident, value) in tag.bytes() {
        if is_rating_ident(ident) {
            if let Some(rating) = parse_rating_bytes(value) {
                return rating;
            }
        }
    }

    0
}

fn set_video_rating_tag(tag: &mut mp4ameta::Tag, rating: u64) {
    let rating_ident =
        mp4ameta::FreeformIdent::new_static("com.video-library-tauri", "RATING");

    if rating == 0 {
        tag.remove_data_of(&rating_ident);
        return;
    }

    tag.set_data(rating_ident, mp4ameta::Data::Utf8(rating.to_string()));
}

fn is_rating_ident(ident: &mp4ameta::DataIdent) -> bool {
    let ident = ident.to_string().to_lowercase();
    ident.contains("rating") || ident.contains("rate") || ident.contains("rtng")
}

fn parse_rating_text(value: &str) -> Option<u64> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_lowercase();
    let number = first_number(trimmed)?;
    let denominator = lower
        .split_once('/')
        .and_then(|(_, right)| first_number(right));

    let normalized = if denominator == Some(5.0) || lower.contains("star") {
        number * 2.0
    } else {
        number
    };

    if !normalized.is_finite() {
        return None;
    }

    Some(normalized.round().clamp(0.0, 10.0) as u64)
}

fn first_number(value: &str) -> Option<f64> {
    let mut buffer = String::new();
    let mut started = false;

    for character in value.chars() {
        if character.is_ascii_digit() || (character == '.' && started) {
            buffer.push(character);
            started = true;
            continue;
        }

        if started {
            break;
        }
    }

    buffer.parse::<f64>().ok()
}

fn parse_rating_bytes(value: &[u8]) -> Option<u64> {
    if let Ok(text) = std::str::from_utf8(value) {
        if let Some(rating) = parse_rating_text(text) {
            return Some(rating);
        }
    }

    let rating = match value.len() {
        1 => u64::from(value[0]),
        2 => u64::from(u16::from_be_bytes([value[0], value[1]])),
        4 => u64::from(u32::from_be_bytes([value[0], value[1], value[2], value[3]])),
        _ => return None,
    };

    (rating <= 10).then_some(rating)
}

fn thumbnail_path(app: &tauri::AppHandle, file_path: &str) -> Result<PathBuf, String> {
    let mut hasher = DefaultHasher::new();
    file_path.hash(&mut hasher);

    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not find app data folder: {error}"))?
        .join("thumbnails")
        .join(format!("{:x}.jpg", hasher.finish())))
}

fn normalize_database(database: &mut VideoDatabase) -> bool {
    let mut changed = false;

    if database.folder_paths.is_empty() && !database.folder_path.is_empty() {
        database.folder_paths.push(database.folder_path.clone());
        changed = true;
    }

    if database.folder_path.is_empty() {
        if let Some(first_path) = database.folder_paths.first() {
            database.folder_path = first_path.clone();
            changed = true;
        }
    }

    let timestamp = current_timestamp();
    for video in &mut database.videos {
        if video.added_at.trim().is_empty() {
            video.added_at = timestamp.clone();
            changed = true;
        }
    }

    changed
}

fn current_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    seconds.to_string()
}

fn save_database(app: &tauri::AppHandle, database: &VideoDatabase) -> Result<(), String> {
    let path = database_path(app)?;
    let mut database = VideoDatabase {
        folder_path: database.folder_path.clone(),
        folder_paths: database.folder_paths.clone(),
        videos: database
            .videos
            .iter()
            .map(|video| VideoFile {
                filename: video.filename.clone(),
                file_path: video.file_path.clone(),
                title: video.title.clone(),
                actor: video.actor.clone(),
                genre: video.genre.clone(),
                date: video.date.clone(),
                backup_date: video.backup_date.clone(),
                backup_location: video.backup_location.clone(),
                notes: video.notes.clone(),
                explicit_content: video.explicit_content,
                resolution: video.resolution.clone(),
                bitrate: video.bitrate.clone(),
                filesize: video.filesize,
                artwork_thumbnail: video.artwork_thumbnail.clone(),
                rating: video.rating,
                play_count: video.play_count,
                added_at: video.added_at.clone(),
            })
            .collect(),
        actor_thumbnails: database.actor_thumbnails.clone(),
        actor_bios: database.actor_bios.clone(),
        actor_social_links: database.actor_social_links.clone(),
        watch_statistics: database.watch_statistics.clone(),
    };
    normalize_database(&mut database);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create database folder: {error}"))?;
    }

    let json = serde_json::to_string_pretty(&database)
        .map_err(|error| format!("Could not create database JSON: {error}"))?;

    fs::write(&path, json).map_err(|error| format!("Could not save database: {error}"))
}

fn increment_play_count(app: &tauri::AppHandle, file_path: &str) -> Result<u64, String> {
    let path = database_path(app)?;

    if !path.exists() {
        return Ok(0);
    }

    let json =
        fs::read_to_string(&path).map_err(|error| format!("Could not read database: {error}"))?;
    let mut database: VideoDatabase = serde_json::from_str(&json)
        .map_err(|error| format!("Could not parse database JSON: {error}"))?;

    let Some(video) = database
        .videos
        .iter_mut()
        .find(|video| video.file_path == file_path)
    else {
        return Ok(0);
    };

    video.play_count += 1;
    let play_count = video.play_count;
    let video_for_stats = video.clone();
    increment_watch_statistics(&mut database.watch_statistics, &video_for_stats);
    save_database(app, &database)?;

    Ok(play_count)
}

fn increment_watch_statistics(statistics: &mut WatchStatistics, video: &VideoFile) {
    statistics.total_watches += 1;

    for actor in split_stat_tags(&video.actor, "Unknown actor") {
        increment_stat_count(&mut statistics.actor_counts, actor);
    }

    for genre in split_stat_tags(&video.genre, "Unknown genre") {
        increment_stat_count(&mut statistics.genre_counts, genre);
    }

    increment_stat_count(
        &mut statistics.year_counts,
        first_year(&video.date).unwrap_or_else(|| "Unknown Year".to_string()),
    );
}

fn split_stat_tags(value: &str, fallback: &str) -> Vec<String> {
    let tags = value
        .split([',', ';', '|', '/'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if tags.is_empty() {
        vec![fallback.to_string()]
    } else {
        tags
    }
}

fn increment_stat_count(counts: &mut HashMap<String, u64>, key: String) {
    *counts.entry(key).or_insert(0) += 1;
}

#[tauri::command]
fn reset_watch_statistics(app: tauri::AppHandle) -> Result<WatchStatistics, String> {
    let mut database = load_required_database(&app)?;
    database.watch_statistics = WatchStatistics::default();
    save_database(&app, &database)?;

    Ok(database.watch_statistics)
}

#[tauri::command]
fn update_video_metadata(
    app: tauri::AppHandle,
    file_path: String,
    title: String,
    actor: String,
    genre: String,
    date: String,
    backup_date: String,
    backup_location: String,
    notes: String,
    explicit_content: bool,
    rating: u64,
    play_count: u64,
) -> Result<VideoFile, String> {
    let path = database_path(&app)?;

    if !path.exists() {
        return Err("Database does not exist.".to_string());
    }

    let json =
        fs::read_to_string(&path).map_err(|error| format!("Could not read database: {error}"))?;
    let mut database: VideoDatabase = serde_json::from_str(&json)
        .map_err(|error| format!("Could not parse database JSON: {error}"))?;

    let updated_video = {
        let Some(video) = database
            .videos
            .iter_mut()
            .find(|video| video.file_path == file_path)
        else {
            return Err("Video was not found in database.".to_string());
        };

        video.title = title;
        video.actor = actor;
        video.genre = genre;
        video.date = date;
        video.backup_date = backup_date;
        video.backup_location = backup_location;
        video.notes = notes;
        video.explicit_content = explicit_content;
        video.rating = rating.min(10);
        video.play_count = play_count;
        VideoFile {
            filename: video.filename.clone(),
            file_path: video.file_path.clone(),
            title: video.title.clone(),
            actor: video.actor.clone(),
            genre: video.genre.clone(),
            date: video.date.clone(),
            backup_date: video.backup_date.clone(),
            backup_location: video.backup_location.clone(),
            notes: video.notes.clone(),
            explicit_content: video.explicit_content,
            resolution: video.resolution.clone(),
            bitrate: video.bitrate.clone(),
            filesize: video.filesize,
            artwork_thumbnail: video.artwork_thumbnail.clone(),
            rating: video.rating,
            play_count: video.play_count,
            added_at: video.added_at.clone(),
        }
    };

    save_database(&app, &database)?;

    Ok(updated_video)
}

#[tauri::command]
fn update_multiple_video_metadata(
    app: tauri::AppHandle,
    file_paths: Vec<String>,
    title: Option<String>,
    actor: Option<String>,
    genre: Option<String>,
    date: Option<String>,
    backup_date: Option<String>,
    backup_location: Option<String>,
    notes: Option<String>,
    explicit_content: Option<bool>,
    rating: Option<u64>,
    play_count: Option<u64>,
) -> Result<Vec<VideoFile>, String> {
    let path = database_path(&app)?;

    if !path.exists() {
        return Err("Database does not exist.".to_string());
    }

    let json =
        fs::read_to_string(&path).map_err(|error| format!("Could not read database: {error}"))?;
    let mut database: VideoDatabase = serde_json::from_str(&json)
        .map_err(|error| format!("Could not parse database JSON: {error}"))?;
    let file_paths = file_paths.into_iter().collect::<HashSet<_>>();
    let mut updated_videos = Vec::new();

    for video in database
        .videos
        .iter_mut()
        .filter(|video| file_paths.contains(&video.file_path))
    {
        if let Some(title) = &title {
            video.title = title.clone();
        }

        if let Some(actor) = &actor {
            video.actor = actor.clone();
        }

        if let Some(genre) = &genre {
            video.genre = genre.clone();
        }

        if let Some(date) = &date {
            video.date = date.clone();
        }

        if let Some(backup_date) = &backup_date {
            video.backup_date = backup_date.clone();
        }

        if let Some(backup_location) = &backup_location {
            video.backup_location = backup_location.clone();
        }

        if let Some(notes) = &notes {
            video.notes = notes.clone();
        }

        if let Some(explicit_content) = explicit_content {
            video.explicit_content = explicit_content;
        }

        if let Some(rating) = rating {
            video.rating = rating.min(10);
        }

        if let Some(play_count) = play_count {
            video.play_count = play_count;
        }

        updated_videos.push(video.clone());
    }

    if updated_videos.is_empty() {
        return Err("No selected videos were found in database.".to_string());
    }

    save_database(&app, &database)?;

    Ok(updated_videos)
}

#[tauri::command]
fn update_video_technical_metadata(
    app: tauri::AppHandle,
    file_path: String,
    resolution: String,
    bitrate: String,
) -> Result<VideoFile, String> {
    let path = database_path(&app)?;

    if !path.exists() {
        return Err("Database does not exist.".to_string());
    }

    let json =
        fs::read_to_string(&path).map_err(|error| format!("Could not read database: {error}"))?;
    let mut database: VideoDatabase = serde_json::from_str(&json)
        .map_err(|error| format!("Could not parse database JSON: {error}"))?;
    normalize_database(&mut database);

    let updated_video = {
        let Some(video) = database
            .videos
            .iter_mut()
            .find(|video| video.file_path == file_path)
        else {
            return Err("Video was not found in database.".to_string());
        };

        video.resolution = resolution;
        video.bitrate = bitrate;
        video.clone()
    };

    save_database(&app, &database)?;

    Ok(updated_video)
}

#[tauri::command]
fn reset_video_database(app: tauri::AppHandle) -> Result<(), String> {
    let path = database_path(&app)?;

    if path.exists() {
        fs::remove_file(&path).map_err(|error| format!("Could not delete database: {error}"))?;
    }

    Ok(())
}

#[tauri::command]
fn save_video_thumbnail(
    app: tauri::AppHandle,
    file_path: String,
    image_bytes: Vec<u8>,
) -> Result<String, String> {
    let thumbnail_path = thumbnail_path(&app, &file_path)?;

    if let Some(parent) = thumbnail_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create thumbnail folder: {error}"))?;
    }

    fs::write(&thumbnail_path, image_bytes)
        .map_err(|error| format!("Could not save thumbnail: {error}"))?;

    let thumbnail_path_string = thumbnail_path.to_string_lossy().to_string();
    let database_path = database_path(&app)?;

    if database_path.exists() {
        let json = fs::read_to_string(&database_path)
            .map_err(|error| format!("Could not read database: {error}"))?;
        let mut database: VideoDatabase = serde_json::from_str(&json)
            .map_err(|error| format!("Could not parse database JSON: {error}"))?;

        if let Some(video) = database
            .videos
            .iter_mut()
            .find(|video| video.file_path == file_path)
        {
            video.artwork_thumbnail = thumbnail_path_string.clone();
            save_database(&app, &database)?;
        }
    }

    Ok(thumbnail_path_string)
}
