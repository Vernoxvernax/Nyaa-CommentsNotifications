use sqlx::{
  sqlite, Pool, Sqlite, Row
};

use crate::NYAA_DATABASE_PATH;
use crate::discord::unix_to_datetime;
use crate::web::{
  NyaaTorrent, NyaaComment, NyaaUpdate, NyaaCommentUpdateType
};
use crate::config::{
  ModuleConfig, ModuleType
};

pub struct Database {
  pub database: Pool<Sqlite>
}

impl Database {
  pub async fn new() -> Result<Self, ()> {
    if let Ok(database) = sqlite::SqlitePoolOptions::new()
      .max_connections(10)
      .connect_with(
        sqlite::SqliteConnectOptions::new()
          .filename(NYAA_DATABASE_PATH.to_string())
          .create_if_missing(true)
        )
    .await {
      return Ok(Database {database});
    }
    eprintln!("Failed to connect or create {}.", *NYAA_DATABASE_PATH);
    Err(())
  }

  pub async fn check_database_connection(&mut self) -> Result<(), ()> {
    if self.database.is_closed() {
      if let Ok(database) = Database::new().await {
        self.database = database.database;
        return Ok(());
      }
      Err(())
    } else {
      Ok(())
    }
  }

  pub async fn use_pool(database_pool: Pool<Sqlite>) -> Result<Self, ()> {
    Ok(Database { database: database_pool })
  }

  pub async fn data_table_exists(&mut self, database_type: String, database_id: &String) -> bool {
    let table_name = format!("_{}_{}", database_type, database_id);
    if ! sqlx::query(format!("SELECT * FROM sqlite_master WHERE type = 'table' AND tbl_name = '{}'", table_name).as_str())
    .fetch_all(&self.database).await.unwrap().is_empty() {
      true
    } else {
      println!("[INF] Creating new table {:?}", table_name);
      sqlx::query(format!(r#"CREATE TABLE {:?} (
        ID INTEGER,
        Domain TEXT NOT NULL,
        Title TEXT NOT NULL,
        Category TEXT NOT NULL,
        Size TEXT NOT NULL,
        Magnet_Link TEXT NOT NULL,
        Upload_Date_Str TEXT NOT NULL,
        Upload_Date_Timestamp INTEGER,
        Seeders INTEGER,
        Leechers INTEGER,
        Completed INTEGER,
        Comments_Amount INTEGER,
        Comments TEXT
      )"#, table_name).as_str()).execute(&self.database).await.unwrap();
      false
    }
  }

  pub async fn update_db_table(&mut self, database_type: String, database_id: &String, mut update: NyaaUpdate) {
    let table_name = format!("_{}_{}", database_type, database_id);
    
    for comment in update.torrent.comments.iter_mut() {
      if unix_to_datetime(comment.date_timestamp)+chrono::Duration::hours(1) <= chrono::Utc::now()-chrono::Duration::minutes(1) {
        comment.update_type = NyaaCommentUpdateType::UNDECIDED;
      }
      // Make it so that comments can't be re-checked when they've already aged more than one hour.
    }

    if update.new_upload {
      sqlx::query(format!(r#"INSERT INTO {:?} (ID, Domain, Title, Category, Size, Magnet_Link, Upload_Date_Str, Upload_Date_Timestamp, Seeders, Leechers, Completed, Comments_Amount, Comments)
        VALUES ({:?}, {:?}, (?), {:?}, {:?}, {:?}, {:?}, {:?}, {:?}, {:?}, {:?}, {:?}, (?))"#,
        table_name,
        update.torrent.id,
        update.torrent.domain,
        update.torrent.category,
        update.torrent.size,
        update.torrent.magnet_link,
        update.torrent.upload_date_str,
        update.torrent.upload_date_timestamp,
        update.torrent.seeders,
        update.torrent.leechers,
        update.torrent.completed,
        update.torrent.comments_amount).as_str()).bind(update.torrent.title).bind(serde_json::to_string(&update.torrent.comments).unwrap())
      .execute(&self.database).await.unwrap();
    } else {
      sqlx::query(format!(r#"UPDATE {:?} SET
      Domain = {:?}, Title = (?), Category = {:?}, Size = {:?}, Magnet_Link = {:?}, Upload_Date_Str = {:?},
      Upload_Date_Timestamp = {:?}, Seeders = {:?}, Leechers = {:?}, Completed = {:?}, Comments_Amount = {:?}, Comments = (?) WHERE ID = {:?}"#,
        table_name,
        update.torrent.domain,
        update.torrent.category,
        update.torrent.size,
        update.torrent.magnet_link,
        update.torrent.upload_date_str,
        update.torrent.upload_date_timestamp,
        update.torrent.seeders,
        update.torrent.leechers,
        update.torrent.completed,
        update.torrent.comments_amount,
        update.torrent.id).as_str()).bind(update.torrent.title).bind(serde_json::to_string(&update.torrent.comments).unwrap())
      .execute(&self.database).await.unwrap();
    }
  }

  pub async fn discord_channel_exists(&mut self, discord_bot_id: &String, discord_channel_id: u64) -> bool {
    if self.discord_table_exists(discord_bot_id).await {
      for module in self.get_discord_channels(discord_bot_id).await {
        if module.discord_channel_id.unwrap() == discord_channel_id {
          return true;
        }
      }
    }
    false
  }

  pub async fn discord_table_exists(&mut self, discord_bot_id: &String) -> bool {
    let table_name = format!("_{}_{}", ModuleType::Discord, discord_bot_id);
    if ! sqlx::query(format!("SELECT * FROM sqlite_master WHERE type = 'table' AND tbl_name = '{}'", table_name).as_str())
    .fetch_all(&self.database).await.unwrap().is_empty() {
      true
    } else {
      println!("[INF] Creating new table {:?}", table_name);
      sqlx::query(format!(r#"CREATE TABLE {:?} (
        Channel TEXT NOT NULL,
        Feed TEXT NOT NULL,
        Active INTEGER,
        Comments INTEGER,
        Uploads INTEGER,
        Retrieve_All_Pages INTEGER,
        Pinged_Role TEXT NOT NULL
      )"#, table_name).as_str()).execute(&self.database).await.unwrap();
      false
    }
  }

  pub async fn add_discord_channel(&mut self, discord_bot_id: &String, discord_channel_id: u64, urls: Vec<String>, collapsed_choice: (bool, bool, bool), pinged_role: String) {
    let index_table_name = format!("_{}_{}", ModuleType::Discord, discord_bot_id);
    let url_string = {
      let mut str: String = String::new();
      for url in urls {
        str.push_str(&(url+","));
      }
      str = str.trim_end_matches(',').to_string();
      str
    };
    let comments_u32 = collapsed_choice.0 as u32;
    let uploads_u32 = collapsed_choice.1 as u32;
    let retrieve_all_pages_u32 = collapsed_choice.2 as u32;

    sqlx::query(format!(r#"INSERT INTO {:?} (Channel, Feed, Active, Comments, Uploads, Retrieve_All_Pages, Pinged_Role)
    VALUES({}, {:?}, {}, {}, {}, {}, {})"#, index_table_name, discord_channel_id, url_string, 1, comments_u32, uploads_u32, retrieve_all_pages_u32, pinged_role).as_str()).execute(&self.database).await.unwrap();
  }

  pub async fn remove_discord_channel(&mut self, discord_bot_id: &String, discord_channel_id: u64) {
    let index_table_name = format!("_{}_{}", ModuleType::Discord, discord_bot_id);
    let channel_table_name = format!("_{}_{}_{}", ModuleType::Discord, discord_bot_id, discord_channel_id);

    sqlx::query(format!(r#"DELETE FROM {:?} WHERE Channel = {:?}"#, index_table_name, discord_channel_id).as_str()).execute(&self.database).await.unwrap();
    sqlx::query(format!(r#"DROP TABLE {:?}"#, channel_table_name).as_str()).execute(&self.database).await.ok();
  }

  pub async fn pause_discord_channel(&mut self, discord_bot_id: &String, discord_channel_id: u64, mode: bool) {
    let index_table_name = format!("_{}_{}", ModuleType::Discord, discord_bot_id);
    let switch = mode as u32;

    sqlx::query(format!(r#"UPDATE {:?} SET Active = {} WHERE Channel = {:?}"#,
      index_table_name, switch, discord_channel_id).as_str()).execute(&self.database
    ).await.unwrap();
  }

  pub async fn get_discord_channels(&mut self, discord_bot_id: &String) -> Vec<ModuleConfig> {
    let table_name = format!("_{}_{}", ModuleType::Discord, discord_bot_id);
    let mut channels: Vec<ModuleConfig> = vec![];
    if self.discord_table_exists(discord_bot_id).await {
      let db = sqlx::query(format!(r#"SELECT * FROM {:?}"#, table_name).as_str()).fetch_all(&self.database).await.unwrap();

      for row in db {
        let channel_str: String = row.get(0);
        let channel: u64 = channel_str.parse().unwrap();
        let feeds_string_list: String = row.get(1);
        let active: bool = row.get(2);
        let comments: bool = row.get(3);
        let uploads: bool = row.get(4);
        let retrieve_all_pages: bool = row.get(5);
        let pinged_role_str: String = row.get(6);
        let pinged_role = pinged_role_str.parse::<u64>().unwrap();
        let feeds: Vec<String> = feeds_string_list.split(',').map(|str| str.to_string()).collect();

        channels.append(&mut vec![ModuleConfig {
          module_type: ModuleType::Discord,
          active,
          feeds: Some(feeds),
          comments: Some(comments),
          uploads: Some(uploads),
          retrieve_all_pages: Some(retrieve_all_pages),
          discord_channel_id: Some(channel),
          smtp_username: None,
          smtp_password: None,
          smtp_domain: None,
          smtp_port: None,
          smtp_subject: None,
          smtp_recipients: None,
          gotify_domain: None,
          gotify_token: None,
          gotify_comment_priority: None,
          gotify_upload_priority: None,
          discord_token: None,
          discord_bot_id: Some(discord_bot_id.to_string()),
          discord_pinged_role: Some(pinged_role),
          discord_bot_activity_type: None,
          discord_bot_activity_text: None
        }]);
      }
    }
    channels
  }

  pub async fn get_torrents_from_db(&mut self, database_type: String, database_id: &String) -> Vec<NyaaTorrent> {
    let table_name = format!("_{}_{}", database_type, database_id);
    let db = sqlx::query(format!(r#"SELECT * FROM {:?}"#, table_name).as_str()).fetch_all(&self.database).await.unwrap();
    let mut torrents: Vec<NyaaTorrent> = vec![];

    for row in db {
      let id: u64 = row.get_unchecked::<f64, _>(0) as u64;
      let domain: String = row.get(1);
      let title_encoded: &String = &row.get(2);
      let title_decoded = html_escape::decode_html_entities(title_encoded);
      let title = title_decoded.replace(r#"\\""#, r#"\""#).to_string();
      let category: String = row.get(3);
      let size: String = row.get(4);
      let magnet_link: String = row.get(5);
      let upload_date_str: String = row.get(6);
      let upload_date_timestamp: f64 = row.get_unchecked::<f64, _>(7);
      let seeders: u64 = row.get_unchecked::<f64, _>(8) as u64;
      let leechers: u64 = row.get_unchecked::<f64, _>(9) as u64;
      let completed: u64 = row.get_unchecked::<f64, _>(10) as u64;
      let comments_amount: u64 = row.get_unchecked::<f64, _>(11) as u64;
      let comments: Vec<NyaaComment> = serde_json::from_str(&row.get_unchecked::<String, _>(12)).unwrap();
      torrents.append(&mut vec![NyaaTorrent {
        uploader: None,
        id,
        domain,
        title,
        category,
        size,
        magnet_link,
        upload_date_str,
        upload_date_timestamp,
        seeders,
        leechers,
        completed,
        comments_amount,
        comments
      }]);
    }

    torrents
  }
}
