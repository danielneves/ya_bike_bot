// TODO think of a better name
use crate::bike_service::Station;
use crate::config::Config;
use chrono::prelude::*;
use serde::{Deserialize, Serialize};
use std::convert::From;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use uuid::Uuid;
const LOW_PERCENTAGE_BIKES: f32 = 0.2; // 20%

fn reply_markup(station: &Station, uuid: &str) -> Option<InlineKeyboardMarkup> {
    let free_bikes = station.free_bikes? as f32;
    let empty_slots = station.empty_slots? as f32;
    let show_warn = (free_bikes / (free_bikes + empty_slots)) <= LOW_PERCENTAGE_BIKES;

    if !show_warn {
        return None;
    };

    let button = InlineKeyboardButton::callback("Alert!".to_string(), uuid.into());
    Some(InlineKeyboardMarkup::default().append_row(vec![button]))
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StationWarn {
    network_href: String,
    free_bikes: u32,
    id: String,
    pub message_id: Option<i32>,
    pub updated_at: DateTime<Utc>,
    pub chat_id: Option<i64>,
}

impl From<&Station> for StationWarn {
    fn from(station: &Station) -> Self {
        let network_href = station.network_href.as_ref().unwrap().into();
        let free_bikes = station.free_bikes.unwrap();
        let id = station.id.clone();
        StationWarn {
            network_href,
            free_bikes,
            id,
            message_id: None,
            chat_id: None,
            updated_at: Utc::now(),
        }
    }
}

pub async fn reply_markups(stations: &[Station]) -> Vec<Option<InlineKeyboardMarkup>> {
    let client = redis::Client::open(crate::config::Config::new().redis_url).unwrap(); // TODO set redis addres to env variable
    let mut con = client.get_async_connection().await.unwrap();
    let uuids: Vec<String> = stations
        .iter()
        .map(|_| Uuid::new_v4().to_simple().to_string())
        .collect();
    let reply_markups: Vec<Option<InlineKeyboardMarkup>> = stations
        .iter()
        .zip(uuids.iter())
        .map(|(station, uuid)| reply_markup(station, &uuid))
        .collect();
    let mut new_pipeline = redis::Pipeline::new();
    let pipeline_set: &mut redis::Pipeline = stations
        .iter()
        .zip(uuids.into_iter())
        .map(|(station, uuid)| {
            dbg!(&uuid);
            let station_warn: StationWarn = station.into();
            let station_warn = serde_json::to_string(&station_warn).unwrap();
            (uuid, station_warn)
        })
        .fold(&mut new_pipeline, |pipe, (uuid, station)| {
            pipe.set(&uuid, station).ignore().expire(uuid, 60 * 60 * 12) // half day
        });
    pipeline_set.query_async::<_, ()>(&mut con).await.unwrap();
    // con.set_multiple::<String, String, ()>(&sets[..])
    //     .await
    //     .unwrap();
    reply_markups
}

pub async fn check_active_warn_stations(bot: Arc<Bot>) {
    let client = redis::Client::open(Config::new().redis_url).unwrap(); // TODO set redis addres to env variable
    let mut con = client.get_async_connection().await.unwrap();
    let keys: Vec<String> = redis::AsyncCommands::keys(&mut con, "ACTIVE*")
        .await
        .unwrap();
    dbg!(&keys);
    let pipeline = redis::Pipeline::new();
    let data: Vec<String> = keys
        .iter()
        .fold(pipeline, |mut pipe, key| pipe.get(key).to_owned())
        .atomic()
        .query_async(&mut con)
        .await
        .unwrap();
    let data: Vec<StationWarn> = data
        .into_iter()
        .map(|d| serde_json::from_str(&d).unwrap())
        .collect();
    for d in data.iter() {
        let chat_id = d.chat_id.unwrap();
        let message_id = d.message_id.unwrap();
        bot.send_message(chat_id, "Test message")
            .reply_to_message_id(message_id)
            .send()
            .await
            .log_on_error()
            .await;
    }
}
