use crate::store::Result;
use chrono::{Duration, NaiveDate};
use rusqlite::{params, params_from_iter, types::Value as SqlValue, Connection};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub fn open_readonly(path: &Path) -> Result<Connection> {
    if !path.is_file() {
        return Err("Connect TrainingPeaks and import your history to get started.".into());
    }
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    db.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    Ok(db)
}

pub fn open(path: &Path, create: bool) -> Result<Connection> {
    if !create && !path.is_file() {
        return Err(
            "No database yet. Sign in and import your history, or select an existing database."
                .into(),
        );
    }
    if create {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let db = Connection::open(path).map_err(|e| e.to_string())?;
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
        .map_err(|e| e.to_string())?;
    db.execute_batch(include_str!("schema.sql"))
        .map_err(|e| e.to_string())?;
    db.execute_batch(include_str!("analytics.sql"))
        .map_err(|e| e.to_string())?;
    let columns = query(&db, "PRAGMA table_info(activity_files)", &[])?;
    if !columns.iter().any(|r| r["name"] == "file_format") {
        db.execute_batch(
            "ALTER TABLE activity_files ADD COLUMN file_format TEXT NOT NULL DEFAULT 'FIT'",
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(db)
}

pub fn query(db: &Connection, sql: &str, values: &[SqlValue]) -> Result<Vec<Value>> {
    let mut statement = db.prepare(sql).map_err(|e| e.to_string())?;
    let names: Vec<String> = statement
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            let mut result = Map::new();
            for (index, name) in names.iter().enumerate() {
                let value = match row.get_ref(index)? {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => json!(n),
                    rusqlite::types::ValueRef::Real(n) => json!(n),
                    rusqlite::types::ValueRef::Text(s) => json!(String::from_utf8_lossy(s)),
                    rusqlite::types::ValueRef::Blob(_) => json!("[binary content]"),
                };
                result.insert(name.clone(), value);
            }
            Ok(Value::Object(result))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
pub fn date(value: &str) -> Result<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| "Use a valid YYYY-MM-DD date".to_string())?;
    if parsed.to_string() != value {
        return Err("Use YYYY-MM-DD".into());
    }
    Ok(parsed)
}
pub fn argument(args: &[String], key: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].clone())
}

pub fn refresh_search(db: &mut Connection) -> Result<()> {
    let signature: String = db
        .query_row(
            "SELECT COUNT(*) || ':' || COALESCE(MAX(updated_at),'') FROM workouts",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let current: Option<String> = db
        .query_row(
            "SELECT value FROM mcp_metadata WHERE key='workout_search_signature'",
            [],
            |r| r.get(0),
        )
        .ok();
    if current.as_deref() == Some(&signature) {
        return Ok(());
    }
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute_batch("DELETE FROM workout_search;
        INSERT INTO workout_search(workout_id,workout_date,workout_type,title,description,athlete_comments,coach_comments)
        SELECT id,workout_date,workout_type,title,description,COALESCE(athlete_comments,''),COALESCE(coach_comments,'') FROM workout_metrics;").map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO mcp_metadata(key,value,updated_at) VALUES ('workout_search_signature',?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at", params![signature,now()]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn overview(db: &Connection) -> Result<Value> {
    let mut result = query(db, "SELECT COUNT(*) AS workouts, MIN(workout_date) AS first_date, MAX(workout_date) AS last_date FROM workouts", &[])?.remove(0);
    for sql in [
        "SELECT COUNT(*) AS logical_files FROM files",
        "SELECT COUNT(*) AS indexed_files FROM activity_files",
        "SELECT COUNT(*) AS saved_plans FROM training_plans",
    ] {
        if let Some(row) = query(db, sql, &[])?.first().and_then(Value::as_object) {
            result.as_object_mut().unwrap().extend(row.clone());
        }
    }
    result["workoutTypeCounts"] = json!(query(db,"SELECT workout_type, COUNT(*) AS workouts FROM workouts GROUP BY workout_type ORDER BY workouts DESC",&[])?);
    result["athletes"] = json!(query(
        db,
        "SELECT athlete_id,COUNT(*) AS workouts FROM workouts GROUP BY athlete_id",
        &[]
    )?);
    Ok(result)
}

pub fn overview_cli(args: &[String]) -> Result<()> {
    let path = argument(args, "--database").ok_or("--database is required")?;
    println!("{}", overview(&open(Path::new(&path), false)?)?);
    Ok(())
}

pub fn filters(input: &Value, alias: &str) -> Result<(String, Vec<SqlValue>)> {
    let mut conditions = vec!["1=1".to_owned()];
    let mut params = vec![];
    for (field, op) in [("startDate", ">="), ("endDate", "<=")] {
        if let Some(value) = input[field].as_str() {
            date(value)?;
            conditions.push(format!("{alias}workout_date {op} ?"));
            params.push(value.to_owned().into());
        }
    }
    if let (Some(start), Some(end)) = (input["startDate"].as_str(), input["endDate"].as_str()) {
        if start > end {
            return Err("endDate must be on or after startDate".into());
        }
    }
    if let Some(types) = input["workoutTypes"].as_array() {
        if types.len() > 20 {
            return Err("At most 20 workout types are supported".into());
        }
        if !types.is_empty() {
            conditions.push(format!(
                "{alias}workout_type IN ({})",
                vec!["?"; types.len()].join(",")
            ));
            for kind in types {
                params.push(
                    kind.as_str()
                        .ok_or("Workout types must be strings")?
                        .to_owned()
                        .into(),
                );
            }
        }
    }
    Ok((conditions.join(" AND "), params))
}

pub fn summarize(db: &Connection, input: &Value) -> Result<Value> {
    let (where_sql, params) = filters(input, "")?;
    let group = input["groupBy"].as_str().unwrap_or("none");
    let expression = match group {
        "none" => "'all'", "week" => "date(workout_date,'-' || ((CAST(strftime('%w',workout_date) AS INTEGER)+6)%7) || ' days')",
        "month" => "strftime('%Y-%m',workout_date)", "type" => "workout_type", _ => return Err("Unknown grouping".into()),
    };
    let sql = format!("SELECT {expression} AS period, COUNT(*) AS workout_count,
        SUM(CASE WHEN COALESCE(duration_hours,0)>0 OR COALESCE(distance_meters,0)>0 THEN 1 ELSE 0 END) AS completed_count,
        ROUND(SUM(COALESCE(duration_hours,0)),2) AS duration_hours, ROUND(SUM(COALESCE(planned_duration_hours,0)),2) AS planned_duration_hours,
        ROUND(SUM(COALESCE(distance_meters,0))/1000,2) AS distance_km, ROUND(SUM(COALESCE(planned_distance_meters,0))/1000,2) AS planned_distance_km,
        ROUND(SUM(COALESCE(tss,0)),1) AS tss, ROUND(AVG(NULLIF(intensity_factor,0)),3) AS avg_intensity_factor,
        ROUND(AVG(NULLIF(heart_rate_average,0)),1) AS avg_heart_rate, ROUND(AVG(NULLIF(power_average,0)),1) AS avg_power,
        ROUND(AVG(NULLIF(rpe,0)),1) AS avg_rpe FROM workout_metrics WHERE {where_sql} {} ORDER BY period",
        if group=="none" { "" } else { "GROUP BY period" });
    Ok(json!({"filters":input,"groups":query(db,&sql,&params)?}))
}

pub fn search(db: &Connection, input: &Value) -> Result<Value> {
    let (mut where_sql, mut params) = filters(input, "w.")?;
    let text = input["query"].as_str().unwrap_or("").trim();
    let mut source = "workout_metrics w";
    let mut order = "w.workout_date DESC,w.id DESC";
    if !text.is_empty() {
        if text.len() > 2000 {
            return Err("Search query is too long".into());
        }
        let tokens: Vec<_> = text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .take(24)
            .collect();
        if tokens.is_empty() {
            return Err("Search requires words or numbers".into());
        }
        params.push(
            tokens
                .iter()
                .map(|s| format!("\"{s}\"*"))
                .collect::<Vec<_>>()
                .join(" AND ")
                .into(),
        );
        where_sql.push_str(" AND workout_search MATCH ?");
        source = "workout_search JOIN workout_metrics w ON w.id=CAST(workout_search.workout_id AS INTEGER)";
        order = "bm25(workout_search),w.workout_date DESC";
    }
    let limit = input["limit"].as_i64().unwrap_or(20).clamp(1, 100);
    let offset = input["offset"].as_i64().unwrap_or(0).max(0);
    params.extend([limit.into(), offset.into()]);
    let rows=query(db,&format!("SELECT w.id,w.workout_date,w.title,w.workout_type,w.duration_hours,ROUND(w.distance_meters/1000,2) AS distance_km,w.tss,w.rpe,w.feeling FROM {source} WHERE {where_sql} ORDER BY {order} LIMIT ? OFFSET ?"),&params)?;
    Ok(json!({"count":rows.len(),"limit":limit,"offset":offset,"workouts":rows}))
}

pub fn training_load(db: &Connection, end: &str, days: i64) -> Result<Value> {
    let end = date(end)?;
    let days = days.clamp(7, 365);
    let start = end - Duration::days(days + 84);
    let visible = end - Duration::days(days - 1);
    let rows=query(db,"SELECT substr(workout_date,1,10) AS date,SUM(COALESCE(tss,0)) AS tss,SUM(COALESCE(duration_hours,0)) AS hours FROM workout_metrics WHERE workout_date BETWEEN ? AND ? GROUP BY substr(workout_date,1,10)",&[start.to_string().into(),end.to_string().into()])?;
    let mut atl = 0.0;
    let mut ctl = 0.0;
    let mut daily = vec![];
    let mut current = start;
    let round = |n: f64| (n * 10.0).round() / 10.0;
    while current <= end {
        let key = current.to_string();
        let row = rows.iter().find(|row| row["date"] == key);
        let tss = row.and_then(|r| r["tss"].as_f64()).unwrap_or(0.0);
        let form = ctl - atl;
        atl += (tss - atl) / 7.0;
        ctl += (tss - ctl) / 42.0;
        if current >= visible {
            daily.push(json!({"date":key,"tss":round(tss),"durationHours":row.and_then(|r|r["hours"].as_f64()).unwrap_or(0.0),"acuteLoad7d":round(atl),"chronicLoad42d":round(ctl),"form":round(form)}));
        }
        current += Duration::days(1);
    }
    let mut latest = daily.last().cloned().unwrap_or(json!({}));
    latest["sevenDayChronicRamp"] = if daily.len() >= 8 {
        json!(round(
            ctl - daily[daily.len() - 8]["chronicLoad42d"]
                .as_f64()
                .unwrap_or(0.0)
        ))
    } else {
        Value::Null
    };
    Ok(
        json!({"methodology":"Daily TSS with 7-day acute and 42-day chronic exponential averages, initialized to zero 84 days before the visible range; form uses prior-day chronic minus acute load. Missing TSS is treated as zero.","endDate":end.to_string(),"days":days,"current":latest,"daily":daily}),
    )
}

pub fn activity(db: &Connection, file_id: i64, maximum: usize) -> Result<Value> {
    let (path, mut bytes): (String,Vec<u8>) = db.query_row("SELECT f.relative_path,b.content FROM files f JOIN file_blobs b ON b.id=f.blob_id WHERE f.id=?",[file_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_| "Activity file not found".to_string())?;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut output = vec![];
        flate2::read::GzDecoder::new(bytes.as_slice())
            .take(128 * 1024 * 1024 + 1)
            .read_to_end(&mut output)
            .map_err(|e| e.to_string())?;
        if output.len() > 128 * 1024 * 1024 {
            return Err("Activity exceeds the decoding limit".into());
        }
        bytes = output;
    }
    if bytes.get(8..12) == Some(b".FIT".as_slice()) {
        let records =
            fitparser::from_bytes(&bytes).map_err(|e| format!("FIT decoding failed: {e}"))?;
        let mut sessions = vec![];
        let mut laps = vec![];
        let mut samples = vec![];
        for record in records {
            let kind = record.kind().to_string();
            let value = Value::Object(
                record
                    .fields()
                    .iter()
                    .map(|field| {
                        (
                            field.name().to_owned(),
                            serde_json::to_value(field.value()).unwrap_or(Value::Null),
                        )
                    })
                    .collect(),
            );
            match kind.as_str() {
                "session" => sessions.push(value),
                "lap" => laps.push(value),
                "record" => samples.push(value),
                _ => {}
            }
        }
        let total = samples.len();
        let maximum = maximum.min(500);
        let step = if maximum == 0 {
            total.max(1)
        } else {
            total.div_ceil(maximum).max(1)
        };
        let samples: Vec<_> = samples.into_iter().step_by(step).take(maximum).collect();
        let lap_count = laps.len();
        return Ok(
            json!({"fileId":file_id,"relativePath":path,"format":"FIT","sessions":sessions,"laps":laps.into_iter().take(200).collect::<Vec<_>>(),"totalLapCount":lap_count,"totalRecordCount":total,"samples":samples}),
        );
    }
    xml_activity(file_id, &path, &bytes, maximum)
}

fn xml_activity(file_id: i64, path: &str, bytes: &[u8], maximum: usize) -> Result<Value> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<String> = vec![];
    let mut point = Map::new();
    let mut lap = Map::new();
    let mut points = vec![];
    let mut laps = vec![];
    let mut format = "UNKNOWN";
    let mut sport = None;
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if name == "TrainingCenterDatabase" {
                    format = "TCX";
                }
                if name == "gpx" {
                    format = "GPX";
                }
                if name == "Activity" {
                    sport = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"Sport")
                        .map(|a| String::from_utf8_lossy(&a.value).to_lowercase());
                }
                if name == "Trackpoint" || name == "trkpt" {
                    point = Map::new();
                    for attr in e.attributes().flatten() {
                        if let Ok(value) = String::from_utf8_lossy(&attr.value).parse::<f64>() {
                            point.insert(
                                String::from_utf8_lossy(attr.key.as_ref()).into(),
                                json!(value),
                            );
                        }
                    }
                }
                if name == "Lap" {
                    lap = Map::new();
                    for attr in e.attributes().flatten() {
                        lap.insert(
                            String::from_utf8_lossy(attr.key.as_ref()).into(),
                            json!(String::from_utf8_lossy(&attr.value)),
                        );
                    }
                }
                stack.push(name);
            }
            Event::Text(e) => {
                if let Some(name) = stack.last() {
                    let text = e.decode().map_err(|e| e.to_string())?.into_owned();
                    let value = text.parse::<f64>().map(|n| json!(n)).unwrap_or(json!(text));
                    let key = if name == "Value" {
                        stack.iter().rev().nth(1).unwrap_or(name)
                    } else {
                        name
                    };
                    if stack.iter().any(|s| s == "Trackpoint" || s == "trkpt") {
                        point.insert(key.clone(), value);
                    } else if stack.iter().any(|s| s == "Lap") {
                        lap.insert(key.clone(), value);
                    }
                }
            }
            Event::End(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if name == "Trackpoint" || name == "trkpt" {
                    points.push(Value::Object(std::mem::take(&mut point)));
                }
                if name == "Lap" {
                    laps.push(Value::Object(std::mem::take(&mut lap)));
                }
                stack.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if format == "UNKNOWN" {
        return Err("Unsupported activity format".into());
    }
    let total = points.len();
    let maximum = maximum.min(500);
    let step = if maximum == 0 {
        total.max(1)
    } else {
        total.div_ceil(maximum).max(1)
    };
    let start = points
        .first()
        .and_then(|p| p.get("Time").or_else(|| p.get("time")))
        .cloned()
        .or_else(|| laps.first().and_then(|p| p.get("StartTime")).cloned());
    let lap_count = laps.len();
    let sum_laps = |key: &str| {
        let values: Vec<_> = laps.iter().filter_map(|lap| lap[key].as_f64()).collect();
        if values.is_empty() {
            Value::Null
        } else {
            json!(values.iter().sum::<f64>())
        }
    };
    let session = json!({"start_time":start,"sport":sport,"total_timer_time":sum_laps("TotalTimeSeconds"),"total_distance":sum_laps("DistanceMeters")});
    Ok(
        json!({"fileId":file_id,"relativePath":path,"format":format,"sessions":[session],"laps":laps.into_iter().take(200).collect::<Vec<_>>(),"totalLapCount":lap_count,"totalRecordCount":total,"samples":points.into_iter().step_by(step).take(maximum).collect::<Vec<_>>()}),
    )
}

pub async fn index_activities(
    db: &mut Connection,
    timezone: &str,
    mut report: impl FnMut(Value),
) -> Result<()> {
    let timezone: chrono_tz::Tz = timezone.parse().map_err(|_| "Invalid activity timezone")?;
    let files=query(db,"SELECT f.id,f.relative_path,f.athlete_id FROM files f LEFT JOIN activity_files a ON a.file_id=f.id WHERE f.extension IN ('gz','fit','tcx','gpx') AND a.file_id IS NULL ORDER BY f.id",&[])?;
    let total = files.len();
    for (index, file) in files.into_iter().enumerate() {
        let id = file["id"].as_i64().ok_or("Invalid file ID")?;
        let result = activity(db, id, 0);
        {
            let tx = db.transaction().map_err(|e| e.to_string())?;
            match result {
                Ok(detail) => {
                    let session = &detail["sessions"][0];
                    let start = session["start_time"]
                        .as_str()
                        .or_else(|| session["timestamp"].as_str());
                    let local = start
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|d| d.with_timezone(&timezone).date_naive().to_string());
                    let sport = session["sport"].as_str().map(str::to_owned).or_else(|| {
                        session["sport"]
                            .as_i64()
                            .map(|n| fitparser::profile::field_types::Sport::from(n).to_string())
                    });
                    let duration = session["total_timer_time"].as_f64();
                    let distance = session["total_distance"].as_f64();
                    let mut matched = None;
                    if let Some(local) = &local {
                        let candidates=query(&tx,"SELECT id,workout_type,duration_hours,distance_meters FROM workout_metrics WHERE athlete_id=? AND substr(workout_date,1,10)=?",&[file["athlete_id"].as_str().unwrap_or("").to_owned().into(),local.clone().into()])?;
                        let score = |row: &Value| {
                            let expected = match sport.as_deref() {
                                Some("running") => "Run",
                                Some("cycling") => "Bike",
                                Some("swimming") => "Swim",
                                Some("walking") => "Walk",
                                _ => "",
                            };
                            let sport_score = if expected.is_empty()
                                || row["workout_type"].as_str() == Some(expected)
                            {
                                0.0
                            } else {
                                10.0
                            };
                            let relative = |a: Option<f64>, b: Option<f64>| match (a, b) {
                                (Some(a), Some(b)) if a > 0.0 && b > 0.0 => {
                                    (a - b).abs() / a.max(b)
                                }
                                _ => 1.0,
                            };
                            sport_score
                                + relative(
                                    duration,
                                    row["duration_hours"].as_f64().map(|n| n * 3600.0),
                                ) * 5.0
                                + relative(distance, row["distance_meters"].as_f64()) * 5.0
                        };
                        let best = candidates
                            .iter()
                            .min_by(|a, b| score(a).total_cmp(&score(b)));
                        if let Some(best) = best.filter(|r| score(r) < 10.0) {
                            matched = best["id"].as_i64();
                        }
                    }
                    tx.execute("INSERT INTO activity_files(file_id,workout_id,relative_path,file_format,fit_valid,crc_valid,start_time,local_date,sport,total_timer_seconds,total_distance_meters,record_count,lap_count,summary_json,indexed_at) VALUES (?,?,?,?,1,?,?,?,?,?,?,?,?,?,?)",
                    params![id,matched,file["relative_path"].as_str(),detail["format"].as_str(),if detail["format"]=="FIT"{1}else{0},start,local,sport,duration,distance,detail["totalRecordCount"].as_i64().unwrap_or(0),detail["totalLapCount"].as_i64().unwrap_or(0),detail.to_string(),now()]).map_err(|e|e.to_string())?;
                    let activity_id = tx.last_insert_rowid();
                    if let Some(laps) = detail["laps"].as_array() {
                        for (index, lap) in laps.iter().enumerate() {
                            tx.execute("INSERT INTO activity_laps(activity_file_id,lap_index,start_time,elapsed_seconds,timer_seconds,distance_meters,avg_heart_rate,max_heart_rate,avg_power,max_power,raw_json) VALUES (?,?,?,?,?,?,?,?,?,?,?)",
                        params![activity_id,index as i64,lap["start_time"].as_str(),lap["total_elapsed_time"].as_f64(),lap["total_timer_time"].as_f64(),lap["total_distance"].as_f64(),lap["avg_heart_rate"].as_f64(),lap["max_heart_rate"].as_f64(),lap["avg_power"].as_f64(),lap["max_power"].as_f64(),lap.to_string()]).map_err(|e|e.to_string())?;
                        }
                    }
                }
                Err(error) => {
                    tx.execute("INSERT INTO activity_files(file_id,relative_path,file_format,fit_valid,crc_valid,decode_errors,indexed_at) VALUES (?,?,'UNKNOWN',0,0,?,?)",params![id,file["relative_path"].as_str(),json!([error]).to_string(),now()]).map_err(|e|e.to_string())?;
                }
            }
            tx.commit().map_err(|e| e.to_string())?;
        }
        if (index + 1) % 25 == 0 || index + 1 == total {
            report(json!({"message":format!("Indexed {} of {total} activity files",index+1)}));
        }
        tokio::task::yield_now().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_dates_are_rejected() {
        assert!(date("2026-02-30").is_err());
        assert!(date("2026-2-2").is_err());
        assert!(date("2024-02-29").is_ok());
    }
}
