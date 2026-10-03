use crate::{data, store::Result};
use chrono::{Datelike, Duration, NaiveDate};
use reqwest::Url;
use rusqlite::{types::Value as SqlValue, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Metric {
    #[default]
    Distance,
    Hours,
    Tss,
}
impl Metric {
    pub fn label(self) -> &'static str {
        match self {
            Self::Distance => "Distance",
            Self::Hours => "Training hours",
            Self::Tss => "Recorded TSS",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Distance => "distance",
            Self::Hours => "hours",
            Self::Tss => "tss",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChartSpec {
    pub start_date: String,
    pub end_date: String,
    #[serde(default)]
    pub metric: Metric,
    #[serde(default = "weekly")]
    pub group_by: String,
    #[serde(default)]
    pub workout_types: Vec<String>,
}
fn weekly() -> String {
    "week".into()
}
impl ChartSpec {
    pub fn validate(&self) -> Result<()> {
        let start = data::date(&self.start_date)?;
        let end = data::date(&self.end_date)?;
        if end < start || (end - start).num_days() > 730 {
            return Err("Chart ranges must be ordered and span at most two years.".into());
        }
        if !["day", "week"].contains(&self.group_by.as_str()) {
            return Err("Charts support day or week grouping.".into());
        }
        if self.workout_types.len() > 20
            || self
                .workout_types
                .iter()
                .any(|s| s.is_empty() || s.len() > 80)
        {
            return Err("Invalid chart sport filter.".into());
        }
        Ok(())
    }
    pub fn url(&self) -> String {
        let mut url = Url::parse("tpgpt://chart").expect("constant URL");
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("startDate", &self.start_date)
                .append_pair("endDate", &self.end_date)
                .append_pair("metric", self.metric.key())
                .append_pair("groupBy", &self.group_by);
            for sport in &self.workout_types {
                query.append_pair("workoutType", sport);
            }
        }
        url.to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnswerAction {
    Chart(ChartSpec),
    Workout(i64),
}
pub fn parse_link(link: &str) -> Result<AnswerAction> {
    if link.len() > 3000 {
        return Err("Interactive link is too long.".into());
    }
    let url = Url::parse(link).map_err(|_| "Invalid interactive link")?;
    if url.scheme() != "tpgpt"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return Err("Invalid interactive link.".into());
    }
    match url.host_str() {
        Some("workout") if url.query().is_none() => {
            let id = url
                .path()
                .trim_start_matches('/')
                .parse::<i64>()
                .map_err(|_| "Invalid workout ID")?;
            if id <= 0 {
                return Err("Invalid workout ID".into());
            }
            Ok(AnswerAction::Workout(id))
        }
        Some("chart") if url.path().is_empty() || url.path() == "/" => {
            let mut spec = ChartSpec {
                start_date: String::new(),
                end_date: String::new(),
                metric: Metric::Distance,
                group_by: weekly(),
                workout_types: vec![],
            };
            let mut seen = std::collections::HashSet::new();
            for (key, value) in url.query_pairs() {
                if key != "workoutType" && !seen.insert(key.to_string()) {
                    return Err("Duplicate chart parameter.".into());
                }
                match key.as_ref() {
                    "startDate" => spec.start_date = value.into_owned(),
                    "endDate" => spec.end_date = value.into_owned(),
                    "groupBy" => spec.group_by = value.into_owned(),
                    "workoutType" => spec.workout_types.push(value.into_owned()),
                    "metric" => {
                        spec.metric = match value.as_ref() {
                            "distance" => Metric::Distance,
                            "hours" => Metric::Hours,
                            "tss" => Metric::Tss,
                            _ => return Err("Unknown chart metric.".into()),
                        }
                    }
                    _ => return Err("Unknown chart parameter.".into()),
                }
            }
            spec.validate()?;
            Ok(AnswerAction::Chart(spec))
        }
        _ => Err("Unknown interactive link.".into()),
    }
}

pub fn link_urls(content: &str) -> Vec<String> {
    let mut links = vec![];
    let mut remaining = content;
    while let Some(start) = remaining.find("tpgpt://") {
        remaining = &remaining[start..];
        let end = remaining
            .find(|c: char| c.is_whitespace() || ['(', ')', ']', '>', '"', '`'].contains(&c))
            .unwrap_or(remaining.len());
        let candidate = &remaining[..end];
        if !links.iter().any(|s| s == candidate) {
            links.push(candidate.to_owned());
        }
        remaining = &remaining[end..];
        if links.len() >= 64 {
            break;
        }
    }
    links
}
pub fn links(content: &str) -> Vec<(String, AnswerAction)> {
    link_urls(content)
        .into_iter()
        .filter_map(|url| parse_link(&url).ok().map(|action| (url, action)))
        .take(8)
        .collect()
}

pub fn workout_references(content: &str) -> Vec<i64> {
    let mut ids = vec![];
    for part in content.split('#').skip(1) {
        let digits: String = part
            .chars()
            .take_while(char::is_ascii_digit)
            .take(19)
            .collect();
        if let Ok(id) = digits.parse::<i64>() {
            if id > 0 && !ids.contains(&id) {
                ids.push(id);
            }
        }
        if ids.len() == 12 {
            break;
        }
    }
    ids
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub workouts: u64,
    pub hours: f64,
    pub distance_km: f64,
    pub tss: f64,
    pub tss_recorded: u64,
    pub active_days: u64,
    pub similar_rows: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub start: String,
    pub end: String,
    pub totals: Totals,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartData {
    pub spec: ChartSpec,
    pub points: Vec<Point>,
    pub totals: Totals,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub as_of: String,
    pub week_start: String,
    pub week: Totals,
    pub previous_week: Totals,
    pub last_seven: Totals,
    pub previous_seven: Totals,
    pub chart: ChartData,
    pub recent: Vec<Value>,
    pub last_workout: Option<String>,
    pub last_import: Option<String>,
    pub total_workouts: u64,
}

fn rows(db: &Connection, spec: &ChartSpec) -> Result<Vec<Value>> {
    spec.validate()?;
    let mut sql = "SELECT id,athlete_id,substr(workout_date,1,10) AS date,title,workout_type,duration_hours,distance_meters,tss FROM workout_metrics WHERE substr(workout_date,1,10) BETWEEN ? AND ? AND (COALESCE(duration_hours,0)>0 OR COALESCE(distance_meters,0)>0 OR COALESCE(tss,0)>0)".to_owned();
    let mut params: Vec<SqlValue> =
        vec![spec.start_date.clone().into(), spec.end_date.clone().into()];
    if !spec.workout_types.is_empty() {
        sql.push_str(&format!(
            " AND workout_type IN ({})",
            vec!["?"; spec.workout_types.len()].join(",")
        ));
        params.extend(spec.workout_types.iter().cloned().map(SqlValue::from));
    }
    sql.push_str(" ORDER BY date,id");
    data::query(db, &sql, &params)
}
fn total(rows: &[&Value]) -> Totals {
    let mut totals = Totals::default();
    let mut days = std::collections::HashSet::new();
    let mut similar = std::collections::HashSet::new();
    for row in rows {
        totals.workouts += 1;
        days.insert(row["date"].as_str());
        totals.hours += row["duration_hours"].as_f64().unwrap_or(0.0);
        totals.distance_km += row["distance_meters"].as_f64().unwrap_or(0.0) / 1000.0;
        if let Some(tss) = row["tss"].as_f64() {
            totals.tss += tss;
            totals.tss_recorded += 1;
        }
        let signature = json!([
            row["athlete_id"],
            row["date"],
            row["title"],
            row["workout_type"],
            row["duration_hours"],
            row["distance_meters"],
            row["tss"]
        ])
        .to_string();
        if !similar.insert(signature) {
            totals.similar_rows += 1;
        }
    }
    totals.active_days = days.len() as u64;
    totals
}
pub fn chart(db: &Connection, spec: ChartSpec) -> Result<ChartData> {
    let rows = rows(db, &spec)?;
    let start = data::date(&spec.start_date)?;
    let end = data::date(&spec.end_date)?;
    let mut cursor = start;
    let mut points = vec![];
    while cursor <= end {
        let until = if spec.group_by == "week" {
            (cursor + Duration::days(6 - cursor.weekday().num_days_from_monday() as i64)).min(end)
        } else {
            cursor
        };
        let a = cursor.to_string();
        let b = until.to_string();
        let selected: Vec<_> = rows
            .iter()
            .filter(|r| {
                r["date"]
                    .as_str()
                    .is_some_and(|d| d >= a.as_str() && d <= b.as_str())
            })
            .collect();
        points.push(Point {
            start: a,
            end: b,
            totals: total(&selected),
        });
        cursor = until + Duration::days(1);
    }
    let totals = total(&rows.iter().collect::<Vec<_>>());
    Ok(ChartData {
        spec,
        points,
        totals,
    })
}
pub fn dashboard(db: &Connection, today: NaiveDate, sport: &str) -> Result<Dashboard> {
    let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let spec = |start: NaiveDate, end: NaiveDate| ChartSpec {
        start_date: start.to_string(),
        end_date: end.to_string(),
        metric: Metric::Distance,
        group_by: weekly(),
        workout_types: if sport.is_empty() {
            vec![]
        } else {
            vec![sport.into()]
        },
    };
    let chart = chart(db, spec(monday - Duration::weeks(7), today))?;
    let recent = rows(db, &spec(today - Duration::days(60), today))?
        .into_iter()
        .rev()
        .take(6)
        .collect();
    let totals = |a, b| rows(db, &spec(a, b)).map(|r| total(&r.iter().collect::<Vec<_>>()));
    let previous_end =
        monday - Duration::days(7) + Duration::days(today.weekday().num_days_from_monday() as i64);
    let coverage = data::query(db,"SELECT MAX(substr(workout_date,1,10)) AS last_workout,COUNT(*) AS total FROM workouts WHERE substr(workout_date,1,10)<=?",&[today.to_string().into()])?;
    let imported = data::query(
        db,
        "SELECT MAX(completed_at) AS last_import FROM export_jobs WHERE status='complete'",
        &[],
    )?;
    Ok(Dashboard {
        as_of: today.to_string(),
        week_start: monday.to_string(),
        week: totals(monday, today)?,
        previous_week: totals(monday - Duration::days(7), previous_end)?,
        last_seven: totals(today - Duration::days(6), today)?,
        previous_seven: totals(today - Duration::days(13), today - Duration::days(7))?,
        chart,
        recent,
        last_workout: coverage[0]["last_workout"].as_str().map(str::to_owned),
        last_import: imported[0]["last_import"].as_str().map(str::to_owned),
        total_workouts: coverage[0]["total"].as_u64().unwrap_or(0),
    })
}
pub fn workout(db: &Connection, id: i64) -> Result<Value> {
    let rows = data::query(db, "SELECT * FROM workout_metrics WHERE id=?", &[id.into()])?;
    let workout = rows
        .first()
        .ok_or("That workout is not in this conversation's local database.")?;
    let activities=data::query(db,"SELECT file_id,relative_path,file_format,record_count,lap_count FROM activity_files WHERE workout_id=? ORDER BY id",&[id.into()])?;
    Ok(json!({"workout":workout,"activities":activities}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interactive_links_are_bounded_and_cannot_address_files_or_remote_services() {
        let spec = ChartSpec {
            start_date: "2026-01-01".into(),
            end_date: "2026-01-15".into(),
            metric: Metric::Hours,
            group_by: weekly(),
            workout_types: vec!["Run".into()],
        };
        assert_eq!(parse_link(&spec.url()).unwrap(), AnswerAction::Chart(spec));
        for link in [
            "file:///etc/passwd",
            "https://trainingpeaks.com/workout/1",
            "tpgpt://workout/-1",
            "tpgpt://workout/1?delete=true",
            "tpgpt://chart?startDate=2026-02-01&endDate=2026-01-01",
            "tpgpt://chart?startDate=2026-01-01&endDate=2026-02-01&sql=DROP",
        ] {
            assert!(parse_link(link).is_err(), "{link}");
        }
        let invalid = "tpgpt://chart?sql=DROP";
        assert_eq!(
            link_urls(&format!("[bad]({invalid})")),
            vec![invalid.to_owned()]
        );
        assert!(links(&format!("[bad]({invalid})")).is_empty());
    }
    #[test]
    fn dashboard_excludes_plans_and_future_rows_and_reports_missing_tss() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("test.sqlite");
        {
            let db = data::open(&path, true).unwrap();
            for (id, day, hours, tss) in [
                (1, "2026-01-05", Some(1.0), Some(50.0)),
                (2, "2026-01-06", Some(0.5), None),
                (3, "2026-01-07", None, None),
                (4, "2026-02-05", Some(3.0), Some(99.0)),
            ] {
                db.execute("INSERT INTO workouts(id,athlete_id,stable_key,workout_date,workout_type,title,raw_json,first_seen_at,updated_at) VALUES (?,'42',?,?,'Run','Fixture','{}','test','test')",rusqlite::params![id,id.to_string(),day]).unwrap();
                for (field, value) in [("TimeTotalInHours", hours), ("TSS", tss)] {
                    if let Some(value) = value {
                        db.execute("INSERT INTO workout_fields(workout_id,field_name,value_text) VALUES (?,?,?)",rusqlite::params![id,field,value.to_string()]).unwrap();
                    }
                }
            }
        }
        let db = data::open_readonly(&path).unwrap();
        let before = std::fs::read(&path).unwrap();
        let home = dashboard(&db, data::date("2026-01-07").unwrap(), "Run").unwrap();
        assert_eq!(home.week.workouts, 2);
        assert_eq!(home.week.hours, 1.5);
        assert_eq!(home.week.tss_recorded, 1);
        assert_eq!(home.week.tss, 50.0);
        assert_eq!(home.chart.points.len(), 8);
        assert_eq!(home.recent.len(), 2);
        assert!(db.execute("DELETE FROM workouts", []).is_err());
        assert_eq!(before, std::fs::read(&path).unwrap());
    }
    #[test]
    fn chart_respects_partial_weeks_sports_and_missing_values_without_deduplicating() {
        let root = tempfile::tempdir().unwrap();
        let db = data::open(&root.path().join("charts.sqlite"), true).unwrap();
        for (id, day, sport, hours, tss) in [
            (1, "2026-01-04", "Run", Some(1.0), Some(40.0)),
            (2, "2026-01-05", "Run", Some(2.0), None),
            (3, "2026-01-05", "Run", Some(2.0), None),
            (4, "2026-01-06", "Bike", Some(3.0), Some(90.0)),
            (5, "2026-01-07", "Strength", None, Some(25.0)),
        ] {
            db.execute("INSERT INTO workouts(id,athlete_id,stable_key,workout_date,workout_type,title,raw_json,first_seen_at,updated_at) VALUES (?,'42',?,?,?,'Fixture','{}','test','test')",rusqlite::params![id,id.to_string(),day,sport]).unwrap();
            for (field, value) in [("TimeTotalInHours", hours), ("TSS", tss)] {
                if let Some(value) = value {
                    db.execute(
                        "INSERT INTO workout_fields VALUES (?,?,?)",
                        rusqlite::params![id, field, value.to_string()],
                    )
                    .unwrap();
                }
            }
        }
        let mut spec = ChartSpec {
            start_date: "2026-01-04".into(),
            end_date: "2026-01-07".into(),
            metric: Metric::Tss,
            group_by: "week".into(),
            workout_types: vec!["Run".into()],
        };
        let result = chart(&db, spec.clone()).unwrap();
        assert_eq!(result.points.len(), 2);
        assert_eq!(
            (
                result.points[0].start.as_str(),
                result.points[0].end.as_str()
            ),
            ("2026-01-04", "2026-01-04")
        );
        assert_eq!(
            (
                result.points[1].start.as_str(),
                result.points[1].end.as_str()
            ),
            ("2026-01-05", "2026-01-07")
        );
        assert_eq!(result.totals.workouts, 3);
        assert_eq!(result.totals.hours, 5.0);
        assert_eq!(result.totals.tss_recorded, 1);
        assert_eq!(result.totals.similar_rows, 1);
        spec.group_by = "day".into();
        spec.workout_types = vec![];
        let result = chart(&db, spec).unwrap();
        assert_eq!(result.points.len(), 4);
        assert_eq!(result.totals.workouts, 5);
        assert_eq!(result.points[3].totals.tss, 25.0);
    }
}
