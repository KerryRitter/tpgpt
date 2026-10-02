use crate::{
    data,
    store::{numeric_id, Result},
};
use base64::Engine;
use chrono::Months;
use reqwest::{Client, Url};
use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub struct ImportConfig {
    pub database: PathBuf,
    pub cache: PathBuf,
    pub athlete: String,
    pub token: String,
    pub end: String,
    pub years: u32,
    pub months: u32,
    pub force: bool,
    pub time_zone: String,
}

pub fn windows(end: &str, years: u32, months: u32) -> Result<Vec<(String, String)>> {
    let end = data::date(end)?;
    if !(1..=50).contains(&years) || !(1..=12).contains(&months) {
        return Err("Use 1–50 years and 1–12 months per request".into());
    }
    let mut result = vec![];
    let mut ago = years * 12;
    while ago > 0 {
        let start = end
            .checked_sub_months(Months::new(ago))
            .ok_or("Date outside supported range")?;
        let next = ago.saturating_sub(months);
        let stop = end
            .checked_sub_months(Months::new(next))
            .ok_or("Date outside supported range")?;
        result.push((start.to_string(), stop.to_string()));
        ago = next;
    }
    Ok(result)
}

pub fn trusted(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url
            .host_str()
            .is_some_and(|h| h == "trainingpeaks.com" || h.ends_with(".trainingpeaks.com"))
}

fn download_url(value: &Value) -> Option<&str> {
    if let Some(s) = value.as_str().filter(|s| s.starts_with("https://")) {
        return Some(s);
    }
    for key in [
        "downloadUrl",
        "downloadURL",
        "fileUrl",
        "fileURL",
        "signedUrl",
        "url",
        "href",
        "location",
    ] {
        if let Some(s) = value[key].as_str() {
            return Some(s);
        }
    }
    for key in ["data", "result", "export", "file"] {
        if let Some(url) = download_url(&value[key]) {
            return Some(url);
        }
    }
    None
}

async fn fetch(client: &Client, start: Url, destination: &Path, token: &str) -> Result<bool> {
    let mut url = start;
    let mut hops = 0;
    'redirect: loop {
        if hops > 8 {
            return Err("Too many export download redirects".into());
        }
        if url.scheme() != "https" {
            return Err("Export download must use HTTPS".into());
        }
        for attempt in 0..5u32 {
            let mut request = client
                .get(url.clone())
                .header("Accept", "application/zip, text/csv, application/json");
            if trusted(&url) {
                request = request
                    .bearer_auth(token)
                    .header("Referer", "https://app.trainingpeaks.com/");
            }
            let response = match request.send().await {
                Ok(r) => r,
                Err(error) => {
                    if attempt == 4 {
                        return Err(format!("Download failed: {}", error.without_url()));
                    }
                    tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
                    continue;
                }
            };
            let status = response.status();
            if status.as_u16() == 204 {
                return Ok(false);
            }
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(format!(
                    "AUTH_EXPIRED: TrainingPeaks returned HTTP {}. Sign in again.",
                    status.as_u16()
                ));
            }
            if status.is_redirection() {
                let next = response
                    .headers()
                    .get("location")
                    .and_then(|s| s.to_str().ok())
                    .ok_or("Redirect missing location")?;
                url = url.join(next).map_err(|e| e.to_string())?;
                hops += 1;
                continue 'redirect;
            }
            if status.as_u16() == 408 || status.as_u16() == 429 || status.is_server_error() {
                if attempt == 4 {
                    return Err(format!(
                        "Export failed after retries: HTTP {}",
                        status.as_u16()
                    ));
                }
                let delay = response
                    .headers()
                    .get("retry-after")
                    .and_then(|s| s.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(1 << attempt)
                    .clamp(1, 60);
                tokio::time::sleep(Duration::from_secs(delay)).await;
                continue;
            }
            if !status.is_success() {
                return Err(format!("Export returned HTTP {}", status.as_u16()));
            }
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|s| s.to_str().ok())
                .unwrap_or("")
                .to_owned();
            let mut response = response;
            let mut file = std::fs::File::create(destination).map_err(|e| e.to_string())?;
            let mut size = 0u64;
            while let Some(bytes) = response
                .chunk()
                .await
                .map_err(|e| e.without_url().to_string())?
            {
                size += bytes.len() as u64;
                if size > 512 * 1024 * 1024 {
                    return Err(
                        "Export exceeds the 512 MiB download limit; use smaller windows".into(),
                    );
                }
                file.write_all(&bytes).map_err(|e| e.to_string())?;
            }
            drop(file);
            if content_type.contains("json") {
                let bytes = std::fs::read(destination).map_err(|e| e.to_string())?;
                let value: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| "Export returned invalid JSON".to_string())?;
                if value["fileName"]
                    .as_str()
                    .is_some_and(|s| s.to_lowercase().ends_with(".zip"))
                    || value["contentType"]
                        .as_str()
                        .is_some_and(|s| s.contains("zip"))
                {
                    let encoded = value["data"].as_str().ok_or("Missing base64 export data")?;
                    let decoded = base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .map_err(|_| "Invalid base64 export".to_string())?;
                    if decoded.is_empty() {
                        return Err("Export returned empty base64 data".into());
                    }
                    std::fs::write(destination, decoded).map_err(|e| e.to_string())?;
                    return Ok(true);
                }
                if let Some(next) = download_url(&value) {
                    url = url.join(next).map_err(|e| e.to_string())?;
                    hops += 1;
                    continue 'redirect;
                }
                // JSON arrays of workout rows can also be imported directly.
                if value.is_array()
                    || ["workouts", "items", "data", "results"]
                        .iter()
                        .any(|key| value[*key].is_array())
                {
                    return Ok(true);
                }
                return Err("Export returned JSON without a download URL or workout rows".into());
            }
            if size == 0 {
                return Err("Export returned an empty response body".into());
            }
            return Ok(true);
        }
        return Err("Export retries exhausted".into());
    }
}

fn normalized(key: &str) -> String {
    key.chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}
fn field(row: &Map<String, Value>, aliases: &[&str]) -> Option<String> {
    row.iter()
        .find(|(key, _)| {
            aliases
                .iter()
                .any(|alias| normalized(alias) == normalized(key))
        })
        .and_then(|(_, value)| text(value))
}
fn text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.trim().to_owned()).filter(|s| !s.is_empty()),
        _ => Some(value.to_string()),
    }
}

pub fn store_workout(
    db: &Connection,
    athlete: &str,
    export: i64,
    file: i64,
    row_number: usize,
    row: &Map<String, Value>,
) -> Result<()> {
    let external = field(
        row,
        &["WorkoutId", "WorkoutPk", "WorkoutKey", "WorkoutGuid", "Id"],
    );
    let raw = serde_json::to_string(row).map_err(|e| e.to_string())?;
    let key = external
        .as_ref()
        .map(|id| format!("id:{id}"))
        .unwrap_or_else(|| format!("sha256:{}", data::hash(raw.as_bytes())));
    let timestamp = data::now();
    db.execute("INSERT INTO workouts(athlete_id,stable_key,external_workout_id,workout_date,title,workout_type,description,raw_json,source_file_id,source_row_number,first_seen_at,updated_at)
        VALUES (?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(athlete_id,stable_key) DO UPDATE SET external_workout_id=excluded.external_workout_id,workout_date=excluded.workout_date,title=excluded.title,workout_type=excluded.workout_type,description=excluded.description,raw_json=excluded.raw_json,source_file_id=excluded.source_file_id,source_row_number=excluded.source_row_number,updated_at=excluded.updated_at",
        params![athlete,key,external,field(row,&["WorkoutDay","WorkoutDate","StartDate","StartTime","Date"]),field(row,&["WorkoutTitle","Title","Name"]),field(row,&["WorkoutType","Sport","Type"]),field(row,&["WorkoutDescription","Description","Notes"]),raw,file,row_number as i64,timestamp,timestamp]).map_err(|e|e.to_string())?;
    let id: i64 = db
        .query_row(
            "SELECT id FROM workouts WHERE athlete_id=? AND stable_key=?",
            params![athlete, key],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    db.execute("DELETE FROM workout_fields WHERE workout_id=?", [id])
        .map_err(|e| e.to_string())?;
    let mut insert = db
        .prepare("INSERT INTO workout_fields(workout_id,field_name,value_text) VALUES (?,?,?)")
        .map_err(|e| e.to_string())?;
    for (key, value) in row {
        insert
            .execute(params![id, key, text(value)])
            .map_err(|e| e.to_string())?;
    }
    db.execute(
        "INSERT OR IGNORE INTO workout_exports(workout_id,export_id) VALUES (?,?)",
        params![id, export],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn store_file(
    db: &Connection,
    athlete: &str,
    export: i64,
    path: &str,
    content: &[u8],
    kind: &str,
) -> Result<usize> {
    let hash = data::hash(content);
    let now = data::now();
    let filename = Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let extension = Path::new(path)
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let mime = match extension.as_str() {
        "csv" => "text/csv",
        "json" => "application/json",
        "fit" => "application/vnd.ant.fit",
        "gpx" => "application/gpx+xml",
        "tcx" => "application/vnd.garmin.tcx+xml",
        _ => "application/octet-stream",
    };
    db.execute(
        "INSERT OR IGNORE INTO file_blobs(sha256,byte_size,content) VALUES (?,?,?)",
        params![hash, content.len() as i64, content],
    )
    .map_err(|e| e.to_string())?;
    let blob: i64 = db
        .query_row("SELECT id FROM file_blobs WHERE sha256=?", [hash], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    db.execute("INSERT INTO files(athlete_id,relative_path,file_name,extension,mime_type,blob_id,first_seen_at,last_seen_at) VALUES (?,?,?,?,?,?,?,?) ON CONFLICT(athlete_id,relative_path,blob_id) DO UPDATE SET last_seen_at=excluded.last_seen_at",params![athlete,path,filename,extension,mime,blob,now,now]).map_err(|e|e.to_string())?;
    let file: i64 = db
        .query_row(
            "SELECT id FROM files WHERE athlete_id=? AND relative_path=? AND blob_id=?",
            params![athlete, path, blob],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    db.execute(
        "INSERT OR IGNORE INTO export_files(export_id,file_id) VALUES (?,?)",
        params![export, file],
    )
    .map_err(|e| e.to_string())?;
    if kind != "workouts" {
        return Ok(0);
    }
    let mut count = 0;
    if extension == "csv" {
        let content = content.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(content);
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(content);
        let headers = reader.headers().map_err(|e| e.to_string())?.clone();
        for (index, record) in reader.records().enumerate() {
            let record = record.map_err(|e| e.to_string())?;
            if record.iter().all(|s| s.trim().is_empty()) {
                continue;
            }
            let row = headers
                .iter()
                .zip(record.iter())
                .map(|(key, value)| (key.to_owned(), json!(value)))
                .collect();
            store_workout(db, athlete, export, file, index + 2, &row)?;
            count += 1;
        }
    } else if extension == "json" {
        let parsed: Value = serde_json::from_slice(content).map_err(|e| e.to_string())?;
        let rows = parsed.as_array().or_else(|| {
            ["workouts", "items", "data", "results"]
                .iter()
                .find_map(|key| parsed[*key].as_array())
        });
        if let Some(rows) = rows {
            for (index, row) in rows.iter().enumerate() {
                if let Some(row) = row.as_object() {
                    store_workout(db, athlete, export, file, index + 1, row)?;
                    count += 1;
                }
            }
        }
    }
    Ok(count)
}

pub fn import_archive(
    db: &mut Connection,
    config: &ImportConfig,
    export: i64,
    path: Option<&Path>,
    kind: &str,
) -> Result<(usize, usize)> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let mut files = 0;
    let mut workouts = 0;
    let mut digest = data::hash(b"");
    tx.execute("DELETE FROM export_files WHERE export_id=?", [export])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM workout_exports WHERE export_id=?", [export])
        .map_err(|e| e.to_string())?;
    if let Some(path) = path {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        digest = data::hash(&bytes);
        if bytes.starts_with(b"PK") {
            let mut zip =
                zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
            if zip.len() > 100_000 {
                return Err("Too many archive entries".into());
            }
            let mut total = 0u64;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
                let name = entry
                    .enclosed_name()
                    .ok_or("Unsafe path in export archive")?
                    .to_string_lossy()
                    .replace('\\', "/");
                if entry.is_dir() {
                    continue;
                }
                if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
                    return Err("Symlink in export archive".into());
                }
                total += entry.size();
                if entry.size() > 128 * 1024 * 1024 || total > 1024 * 1024 * 1024 {
                    return Err("Export expansion exceeds limits; use smaller windows".into());
                }
                let mut content = vec![];
                entry.read_to_end(&mut content).map_err(|e| e.to_string())?;
                workouts += store_file(&tx, &config.athlete, export, &name, &content, kind)?;
                files += 1;
            }
        } else {
            let name = if kind == "workouts" {
                if bytes
                    .iter()
                    .find(|b| !b.is_ascii_whitespace())
                    .is_some_and(|b| *b == b'[' || *b == b'{')
                {
                    "workouts.json"
                } else {
                    "workouts.csv"
                }
            } else {
                "activity.bin"
            };
            workouts += store_file(&tx, &config.athlete, export, name, &bytes, kind)?;
            files = 1;
        }
    }
    tx.execute("UPDATE export_jobs SET status='complete',archive_sha256=?,completed_at=?,error=NULL,file_count=?,workout_count=? WHERE id=?",params![digest,data::now(),files as i64,workouts as i64,export]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok((files, workouts))
}

pub async fn sync(config: ImportConfig, mut report: impl FnMut(Value)) -> Result<Value> {
    if !numeric_id(&config.athlete) || config.token.trim().is_empty() {
        return Err("Sign in and provide a numeric athlete ID".into());
    }
    let windows = windows(&config.end, config.years, config.months)?;
    let mut db = data::open(&config.database, true)?;
    // The analytics server covers one athlete. Avoid mixing accounts silently.
    let athletes = data::query(&db, "SELECT DISTINCT athlete_id FROM workouts", &[])?;
    if athletes
        .iter()
        .any(|row| row["athlete_id"] != config.athlete)
    {
        return Err("This database belongs to another athlete. Select a separate database path in Settings.".into());
    }
    let cache = config.cache.join(&config.athlete);
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let mut complete = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let total = windows.len() * 2;
    for (start, end) in windows {
        for kind in ["workouts", "files"] {
            let label = format!("{kind} {start} to {end}");
            let previous:Option<String>=db.query_row("SELECT status FROM export_jobs WHERE athlete_id=? AND export_kind=? AND window_start=? AND window_end=?",params![config.athlete,kind,start,end],|r|r.get(0)).ok();
            if !config.force && previous.as_deref() == Some("complete") {
                skipped += 1;
                report(
                    json!({"message":format!("Already imported: {label}"),"done":complete+skipped+failed,"total":total}),
                );
                continue;
            }
            let url = format!(
                "https://tpapi.trainingpeaks.com/fitness/v1/export/{}/{kind}/{start}/{end}",
                config.athlete
            );
            db.execute("INSERT INTO export_jobs(athlete_id,export_kind,window_start,window_end,request_url,status,started_at) VALUES (?,?,?,?,?,'started',?) ON CONFLICT(athlete_id,export_kind,window_start,window_end) DO UPDATE SET status='started',started_at=excluded.started_at,error=NULL",params![config.athlete,kind,start,end,url,data::now()]).map_err(|e|e.to_string())?;
            let id:i64=db.query_row("SELECT id FROM export_jobs WHERE athlete_id=? AND export_kind=? AND window_start=? AND window_end=?",params![config.athlete,kind,start,end],|r|r.get(0)).map_err(|e|e.to_string())?;
            let path = cache.join(format!("{kind}-{start}-{end}.export"));
            let pending = path.with_extension("part");
            report(
                json!({"message":format!("Downloading {label}"),"done":complete+skipped+failed,"total":total}),
            );
            let result = async {
                let cached = !config.force
                    && path.is_file()
                    && std::fs::metadata(&path)
                        .map(|m| m.len() > 0)
                        .unwrap_or(false);
                let exists = if cached {
                    true
                } else {
                    let exists = fetch(
                        &client,
                        Url::parse(&url).map_err(|e| e.to_string())?,
                        &pending,
                        &config.token,
                    )
                    .await?;
                    if exists {
                        std::fs::rename(&pending, &path).map_err(|e| e.to_string())?;
                    } else {
                        let _ = std::fs::remove_file(&pending);
                    }
                    exists
                };
                import_archive(&mut db, &config, id, exists.then_some(path.as_path()), kind)
            }
            .await;
            match result {
                Ok((files, workouts)) => {
                    complete += 1;
                    report(
                        json!({"message":format!("Imported {label}: {workouts} workouts, {files} files"),"done":complete+skipped+failed,"total":total}),
                    );
                }
                Err(error) => {
                    let _ = std::fs::remove_file(&pending);
                    let _ = std::fs::remove_file(&path);
                    let error = error.replace(&config.token, "[redacted]");
                    db.execute(
                        "UPDATE export_jobs SET status='failed',completed_at=?,error=? WHERE id=?",
                        params![data::now(), error, id],
                    )
                    .map_err(|e| e.to_string())?;
                    failed += 1;
                    report(
                        json!({"message":format!("Failed {label}: {error}"),"done":complete+skipped+failed,"total":total}),
                    );
                    if error.starts_with("AUTH_EXPIRED") {
                        return Err(error);
                    }
                }
            }
        }
    }
    report(json!({"message":"Building workout search index","done":total,"total":total}));
    data::refresh_search(&mut db)?;
    data::index_activities(&mut db, &config.time_zone, &mut report).await?;
    let result = json!({"completed":complete,"skipped":skipped,"failed":failed,"overview":data::overview(&db)?});
    if failed > 0 {
        return Err(format!("Import finished with {failed} failed windows. Completed windows are saved; sync again to retry."));
    }
    Ok(result)
}

pub fn run_cli(args: &[String]) -> Result<()> {
    let database = data::argument(args, "--database").ok_or("--database is required")?;
    let athlete = data::argument(args, "--athlete-id").ok_or("--athlete-id is required")?;
    let token =
        std::env::var("TRAININGPEAKS_TOKEN").map_err(|_| "Set TRAININGPEAKS_TOKEN".to_string())?;
    let end = data::argument(args, "--end")
        .unwrap_or_else(|| chrono::Local::now().date_naive().to_string());
    let years = data::argument(args, "--years")
        .map(|s| s.parse())
        .transpose()
        .map_err(|_| "Invalid years")?
        .unwrap_or(5);
    let months = data::argument(args, "--months-per-request")
        .map(|s| s.parse())
        .transpose()
        .map_err(|_| "Invalid months")?
        .unwrap_or(3);
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let cache = Path::new(&database)
        .parent()
        .unwrap_or(Path::new("."))
        .join("exports");
    let result = runtime.block_on(sync(
        ImportConfig {
            database: database.into(),
            cache,
            athlete,
            token,
            end,
            years,
            months,
            force: args.iter().any(|s| s == "--force"),
            time_zone: data::argument(args, "--timezone")
                .unwrap_or_else(|| "America/Menominee".into()),
        },
        |event| eprintln!("{}", event["message"].as_str().unwrap_or("")),
    ))?;
    println!("{result}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn month_windows_clamp_leap_days() {
        let windows = windows("2024-02-29", 1, 3).unwrap();
        assert_eq!(windows.len(), 4);
        assert_eq!(windows[0].0, "2023-02-28");
        assert_eq!(windows.last().unwrap().1, "2024-02-29");
    }
    #[test]
    fn bearer_is_only_sent_to_trainingpeaks() {
        assert!(trusted(
            &Url::parse("https://tpapi.trainingpeaks.com/path").unwrap()
        ));
        assert!(!trusted(
            &Url::parse("https://trainingpeaks.com.evil.example/path").unwrap()
        ));
        assert!(!trusted(
            &Url::parse("http://tpapi.trainingpeaks.com/path").unwrap()
        ));
    }
    #[test]
    fn csv_import_deduplicates_and_keeps_all_fields() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("workouts.csv");
        std::fs::write(&path,"WorkoutId,WorkoutDay,WorkoutTitle,WorkoutType,TimeTotalInHours,DistanceInMeters,TSS,Custom Field\n100,2026-01-02,Easy Run,Run,1,10000,55,alpha\n").unwrap();
        let config = ImportConfig {
            database: root.path().join("db.sqlite"),
            cache: root.path().join("cache"),
            athlete: "42".into(),
            token: "test".into(),
            end: "2026-01-01".into(),
            years: 1,
            months: 3,
            force: false,
            time_zone: "America/Menominee".into(),
        };
        let mut db = data::open(&config.database, true).unwrap();
        db.execute("INSERT INTO export_jobs(athlete_id,export_kind,window_start,window_end,request_url,status,started_at) VALUES ('42','workouts','2026-01-01','2026-04-01','test','started','2026-01-01')",[]).unwrap();
        import_archive(&mut db, &config, 1, Some(&path), "workouts").unwrap();
        import_archive(&mut db, &config, 1, Some(&path), "workouts").unwrap();
        data::refresh_search(&mut db).unwrap();
        assert_eq!(data::overview(&db).unwrap()["workouts"], 1);
        assert_eq!(
            data::summarize(&db, &json!({})).unwrap()["groups"][0]["tss"],
            55.0
        );
        assert_eq!(
            data::search(&db, &json!({"query":"Easy"})).unwrap()["count"],
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT value_text FROM workout_fields WHERE field_name='Custom Field'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "alpha"
        );
    }

    #[tokio::test]
    async fn tcx_files_are_indexed_and_linked_to_workouts() {
        let root = tempfile::tempdir().unwrap();
        let mut db = data::open(&root.path().join("db.sqlite"), true).unwrap();
        db.execute("INSERT INTO export_jobs(athlete_id,export_kind,window_start,window_end,request_url,status,started_at) VALUES ('42','workouts','2026-01-01','2026-04-01','test','started','2026-01-01')",[]).unwrap();
        store_file(&db,"42",1,"workouts.csv",b"WorkoutId,WorkoutDay,WorkoutType,TimeTotalInHours,DistanceInMeters\n100,2026-01-02,Run,1,10000\n","workouts").unwrap();
        let xml=br#"<TrainingCenterDatabase><Activities><Activity Sport="Running"><Lap StartTime="2026-01-02T12:00:00Z"><TotalTimeSeconds>3600</TotalTimeSeconds><DistanceMeters>10000</DistanceMeters><Track><Trackpoint><Time>2026-01-02T12:00:00Z</Time><HeartRateBpm><Value>130</Value></HeartRateBpm></Trackpoint><Trackpoint><Time>2026-01-02T13:00:00Z</Time><HeartRateBpm><Value>140</Value></HeartRateBpm></Trackpoint></Track></Lap></Activity></Activities></TrainingCenterDatabase>"#;
        let mut encoded = flate2::write::GzEncoder::new(vec![], flate2::Compression::default());
        encoded.write_all(xml).unwrap();
        store_file(
            &db,
            "42",
            1,
            "activity.tcx.gz",
            &encoded.finish().unwrap(),
            "files",
        )
        .unwrap();
        data::index_activities(&mut db, "America/Menominee", |_| {})
            .await
            .unwrap();
        let index = data::query(
            &db,
            "SELECT file_id,workout_id,record_count FROM activity_files",
            &[],
        )
        .unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index[0]["workout_id"], 1);
        assert_eq!(index[0]["record_count"], 2);
        let detail = data::activity(&db, index[0]["file_id"].as_i64().unwrap(), 1).unwrap();
        assert_eq!(detail["format"], "TCX");
        assert_eq!(detail["samples"].as_array().unwrap().len(), 1);
        assert_eq!(detail["samples"][0]["HeartRateBpm"], 130.0);
    }

    #[test]
    fn unsafe_zip_paths_fail_without_partial_import() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("bad.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("../escape.csv", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"WorkoutId\n1\n").unwrap();
        zip.finish().unwrap();
        let config = ImportConfig {
            database: root.path().join("db.sqlite"),
            cache: root.path().join("cache"),
            athlete: "42".into(),
            token: "test".into(),
            end: "2026-01-01".into(),
            years: 1,
            months: 3,
            force: false,
            time_zone: "America/Menominee".into(),
        };
        let mut db = data::open(&config.database, true).unwrap();
        db.execute("INSERT INTO export_jobs(athlete_id,export_kind,window_start,window_end,request_url,status,started_at) VALUES ('42','workouts','2026-01-01','2026-04-01','test','started','2026-01-01')",[]).unwrap();
        assert!(import_archive(&mut db, &config, 1, Some(&path), "workouts").is_err());
        assert_eq!(data::overview(&db).unwrap()["workouts"], 0);
        assert_eq!(data::overview(&db).unwrap()["logical_files"], 0);
    }
}
