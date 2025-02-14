use std::{
  thread, time::Duration
};
use isahc::{
  prelude::Configurable, RequestExt, http::StatusCode, ReadResponseExt
};
use serde::{
  Deserialize, Serialize
};

use crate::database::Database;
use crate::config::{ModuleConfig, ModuleType};
use crate::discord::unix_to_datetime;
use crate::html::{serialize_feed, serialize_torrent, serialize_user_page};

pub struct Web {
  pub cache_users: Vec<NyaaUser>,
  pub cache_pages: Vec<NyaaPage>
}

#[derive(Debug, Clone)]
pub struct NyaaPage {
  url: String,
  complete: bool,
  torrents: Vec<NyaaTorrent>
}

#[derive(Debug, Clone)]
pub struct NyaaTorrent {
  pub uploader: Option<NyaaUser>,
  pub id: u64,
  pub domain: String,
  pub title: String,
  pub category: String,
  pub size: String,
  pub magnet_link: String,
  pub upload_date_str: String,
  pub upload_date_timestamp: f64,
  pub seeders: u64,
  pub leechers: u64,
  pub completed: u64,
  pub comments_amount: u64,
  pub comments: Vec<NyaaComment>
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NyaaUser {
  pub anonymous: bool,
  pub role: String,
  pub username: String,
  pub avatar: Option<String>,
  pub banned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NyaaComment {
  pub user: NyaaUser,
  pub message: String,
  pub old_message: Option<String>,
  pub uploader: bool,
  pub date_timestamp: f64,
  pub edited_timestamp: Option<f64>,
  pub old_edited_timestamp: Option<f64>,
  pub direct_link: String,
  pub update_type: NyaaCommentUpdateType
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NyaaCommentUpdateType {
  NEW,
  EDITED,
  DELETED,
  UNDECIDED,
  UNCHECKED
}

#[derive(Debug, Clone)]
pub struct NyaaUpdate {
  pub new_upload: bool,
  pub torrent: NyaaTorrent
}

impl Default for Web {
  fn default() -> Self {
    Self::new()
  }
}

impl Web {
  fn new() -> Self {
    Web { cache_users: vec![], cache_pages: vec![] }
  }

  pub async fn get_updates(&mut self, module: &ModuleConfig, module_id: &String, database: &mut Database) -> Vec<NyaaUpdate> {
    let mut updates: Vec<NyaaUpdate> = vec![];
    let mut table_exists: bool = true; // specifically needed for channels with multiple feeds, as everything goes into the same table
    for url in &module.feeds.clone().unwrap() {
      let mut feed = self.search_feed(url, module.retrieve_all_pages.unwrap());
      // Check if table exist
      database.check_database_connection().await.unwrap();
      if database.data_table_exists(module.module_type.to_string(), module_id).await && table_exists {
        let database_torrents = database.get_torrents_from_db(module.module_type.to_string(), module_id).await;
        for torrent in feed.torrents.iter_mut() {
          if let Some(db_torrent) = database_torrents.iter().find(|t| t.id == torrent.id) {
            // Torrent is not new
            if module.comments.unwrap() {
              if db_torrent.comments_amount != torrent.comments_amount {
                // If the current comment amount is 0 but the db one is not, then don't get the comments again.
                if torrent.comments_amount == 0 {
                  let mut update: NyaaTorrent = db_torrent.clone();
                  for comment in update.comments.iter_mut() {
                    comment.update_type = NyaaCommentUpdateType::DELETED;
                  }
                  update.comments_amount = 0;
                  updates.append(&mut vec![NyaaUpdate {
                    new_upload: false,
                    torrent: update
                  }]);
                } else {
                  if torrent.comments.is_empty() {
                    if let Ok(full_torrent) = self.get_torrent(torrent.clone()) {
                      *torrent = full_torrent.clone();
                    } else {
                      continue;
                    }
                  }
                  let mut update: NyaaTorrent = torrent.clone();
                  // find new / edited / deleted comments
                  update.comments = self.find_comment_changes(torrent.clone(), db_torrent.clone());
                  if update.comments.iter().any(|c| (c.update_type != NyaaCommentUpdateType::UNCHECKED) &&
                  (c.update_type != NyaaCommentUpdateType::UNDECIDED)) {
                    updates.append(&mut vec![NyaaUpdate {
                      new_upload: false,
                      torrent: update
                    }]);
                  }
                }
              } else if db_torrent.comments_amount != 0 {
                // Check if there is a comment with the "new" type which is more than one hour old.
                if db_torrent.comments.iter().any(|c| (c.update_type == NyaaCommentUpdateType::UNCHECKED) &&
                (unix_to_datetime(c.date_timestamp)+chrono::Duration::hours(1) <= chrono::Utc::now())) {
                  if torrent.comments.is_empty() {
                    if let Ok(full_torrent) = self.get_torrent(torrent.clone()) {
                      *torrent = full_torrent.clone();
                    } else {
                      continue;
                    }
                  }
                  let mut update: NyaaTorrent = torrent.clone();
                  // find new / edited / deleted comments
                  update.comments = self.find_comment_changes(torrent.clone(), db_torrent.clone());
                  if update.comments.iter().any(|c| c.update_type != NyaaCommentUpdateType::UNDECIDED) {
                    updates.append(&mut vec![NyaaUpdate {
                      new_upload: false,
                      torrent: update
                    }]);
                  }
                }
              }
            }
          } else {
            // Torrent is new
            // a few complicated if statements, because it's possible the torrent is cached
            
            if (torrent.comments.is_empty() && torrent.comments_amount != 0) && module.comments.unwrap() ||
            (module.module_type == ModuleType::Discord && torrent.uploader.is_none()) {
              if let Ok(full_torrent) = self.get_torrent(torrent.clone()) {
                *torrent = full_torrent;
                for comment in torrent.comments.iter_mut() {
                  comment.update_type = NyaaCommentUpdateType::NEW;
                }
              } else {
                continue;
              }
            }

            // see if uploader needed (and see if it has been retrieved above already)
            if module.module_type == ModuleType::Discord {
              if torrent.uploader.clone().unwrap().anonymous {
                torrent.uploader = Some(NyaaUser {
                  anonymous: true,
                  role: "User".to_string(),
                  username: "Anonymous".to_string(),
                  avatar: Some(torrent.domain.clone()+"static/img/avatar/default.png"),
                  banned: false
                });
              } else if let Ok(avatar) = self.get_user_avatar(torrent.clone()) {
                let uploader = torrent.uploader.clone().unwrap();
                torrent.uploader = Some(NyaaUser {
                  anonymous: false,
                  username: uploader.username,
                  role: uploader.role,
                  avatar: Some(avatar),
                  banned: uploader.banned
                });
              } else {
                continue;
              }
            }

            updates.append(&mut vec![NyaaUpdate {
              new_upload: true,
              torrent: torrent.clone()
            }]);
          }
        }
      } else {
        // index all torrents but not as new torrents
        for torrent in feed.torrents.iter_mut() {
          if (torrent.comments_amount != 0) && module.comments.unwrap() {
            if let Ok(full_torrent) = self.get_torrent(torrent.to_owned()) {
              torrent.comments = full_torrent.comments;
            }
          }

          database.update_db_table(module.module_type.to_string(), module_id,
          NyaaUpdate {
            new_upload: true,
            torrent: torrent.clone()
          }).await;

          table_exists = false;
        }
      }
      // here put all of it into the cache
      self.cache_pages.append(&mut vec![NyaaPage {
        url: url.to_string(),
        complete: module.retrieve_all_pages.unwrap(),
        torrents: feed.torrents
      }]);
    }
    updates
  }

  fn find_comment_changes(&mut self, full_torrent: NyaaTorrent, db_torrent: NyaaTorrent) -> Vec<NyaaComment> {
    let mut update = vec![];

    // new comment (based on the username and initial timestamp) [looking for negative]
    for mut new_comment in full_torrent.comments.clone() {
      // put all of the users into the cache
      self.cache_users.append(&mut vec![new_comment.user.clone()]);

      let mut new_flag: bool = true;
      for old_comment in db_torrent.comments.clone() {
        if (new_comment.user.username == old_comment.user.username) &&
        (new_comment.date_timestamp == old_comment.date_timestamp) {
          new_flag = false;
          break;
        }
      }
      if new_flag {
        new_comment.update_type = NyaaCommentUpdateType::NEW;
        update.append(&mut vec![new_comment]);
      }
    }

    // deleted comment (based on the username and initial timestamp) [looking for negative]
    for mut old_comment in db_torrent.comments.clone() {
      let mut deleted_flag: bool = true;
      for new_comment in full_torrent.comments.clone() {
        if (new_comment.user.username == old_comment.user.username) &&
        (new_comment.date_timestamp == old_comment.date_timestamp) {
          deleted_flag = false;
        }
      }
      if deleted_flag {
        old_comment.update_type = NyaaCommentUpdateType::DELETED;
        update.append(&mut vec![old_comment]);
        break;
      }
    }

    // edited comment (based on the username, initial timestamp and edited_timestamp) [looking for positive]
    for new_comment in full_torrent.comments.clone() {
      let mut edited: bool = false;
      for old_comment in db_torrent.comments.clone() {
        if (new_comment.edited_timestamp.is_some()) &&
        (new_comment.user.username == old_comment.user.username) &&
        (new_comment.date_timestamp == old_comment.date_timestamp) &&
        (new_comment.edited_timestamp != old_comment.edited_timestamp) &&
        (new_comment.message != old_comment.message) {
          edited = true;
          update.append(&mut vec![NyaaComment {
            user: new_comment.user,
            message: new_comment.message,
            old_message: Some(old_comment.message),
            uploader: new_comment.uploader,
            date_timestamp: new_comment.date_timestamp,
            edited_timestamp: new_comment.edited_timestamp,
            old_edited_timestamp: old_comment.edited_timestamp,
            direct_link: new_comment.direct_link,
            update_type: NyaaCommentUpdateType::EDITED
          }]);
          break;
        }
      }
      if edited {
        break;
      }
    }

    // add expired' unchecked comments to the array since they would otherwise not trigger an update
    for old_comment in db_torrent.comments {
      if (old_comment.update_type == NyaaCommentUpdateType::UNCHECKED) &&
      (!update.iter().any(|c| {
        c.user.username == old_comment.user.username &&
        c.uploader == old_comment.uploader &&
        c.date_timestamp == old_comment.date_timestamp
      })) {
        update.append(&mut vec![old_comment]);
      }
    }

    for comment in full_torrent.comments {
      if !update.iter().any(|c| {
        c.user.username == comment.user.username &&
        c.uploader == comment.uploader &&
        c.date_timestamp == comment.date_timestamp
      }) {
        update.append(&mut vec![comment]);
      }
    }

    if update.is_empty() {
      eprintln!("Incorrect comment amount detected in database.\n\
      Reset the database if releases are constantly being downloaded without any updates!");
    }

    update
  }

  fn get_user_avatar(&mut self, torrent: NyaaTorrent) -> Result<String, ()> {
    let uploader = torrent.uploader.unwrap();
    for cached_user in self.cache_users.clone() {
      if cached_user.username == uploader.username {
        if let Some(avatar) = cached_user.avatar {
          return Ok(avatar);
        }
      }
    }

    let nyaa_url = format!("{}user/{}", torrent.domain, uploader.username);
    if let Ok(html) = get_nyaa(&nyaa_url) {
      let avatar = serialize_user_page(&html, &torrent.domain);
      self.cache_users.append(&mut vec![NyaaUser {
        anonymous: false,
        role: uploader.role,
        username: uploader.username,
        avatar: Some(avatar.clone()),
        banned: uploader.banned
      }]);
      Ok(avatar)
    } else {
      Err(())
    }
  }

  fn get_torrent(&mut self, torrent: NyaaTorrent) -> Result<NyaaTorrent, ()> {
    // check if comments have already been loaded into self if not then get it
    for page in self.cache_pages.clone() {
      for cached_torrent in page.torrents {
        if (cached_torrent.id == torrent.id) && (!cached_torrent.comments.is_empty()) {
          return Ok(cached_torrent);
        }
      }
    }
    
    // serialize torrent page
    let nyaa_url = format!("{}view/{}", torrent.domain, torrent.id);
    if let Ok(html) = get_nyaa(&nyaa_url) {
      let (uploader, comments) = serialize_torrent(&html, nyaa_url, &torrent.domain);
      let mut full_torrent = torrent;
      full_torrent.comments = comments;
      full_torrent.uploader = uploader;
      Ok(full_torrent)
    } else {
      Err(())
    }
  }

  pub fn search_feed(&mut self, url: &String, complete: bool) -> NyaaPage {
    let mut cache_complete: bool = false;
    let mut torrents: Vec<NyaaTorrent> = vec![];
    for page in self.cache_pages.clone() {
      if page.url == *url {
        if page.complete && ! complete {
          for page_torrent in page.torrents {
            if !torrents.iter().any(|t| page_torrent.id == t.id) {
              torrents.append(&mut vec![page_torrent]);
            }
            if torrents.len() == 85 {
              break;
            }
          }
          break;
        } else {
          cache_complete = true;
          for page_torrent in page.torrents.clone() {
            if !torrents.iter().any(|t| page_torrent.id == t.id) {
              torrents.append(&mut vec![page_torrent]);
            }
          }
          if !complete {
            break;
          }
        }
      }
    }

    let mut url = if url.contains('?') {
      format!("{}&", url)
    } else if url.ends_with("nyaa.si") {
      format!("{}/?", url)
    } else {
      format!("{}?", url)
    };

    url = url.replace("http:", "https:");

    if torrents.is_empty() {
      torrents.append(&mut self.get_feed(&url, complete, false));
    } else if complete && ! cache_complete {
      torrents.append(&mut self.get_feed(&url, complete, true));
    }

    NyaaPage {
      url: url.to_string(),
      complete,
      torrents
    }
  }

  fn get_feed(&mut self, url: &String, complete: bool, skip_first_page: bool) -> Vec<NyaaTorrent> {
    let domain = get_domain(url);
    let mut torrents: Vec<NyaaTorrent> = vec![];
    let mut page_number = if skip_first_page { 2 } else { 1 };
    loop {
      let nyaa_url = format!("{}p={}", url, page_number);
      if let Ok(html) = get_nyaa(&nyaa_url) {
        if let Ok((mut feed, continuing)) = serialize_feed(html, &domain) {
          torrents.append(&mut feed);
          if ! continuing || ! complete {
            break;
          }
        }
      } else {
        return torrents;
      }

      page_number += 1;
    }

    torrents
  }
}

fn get_nyaa(nyaa_url: &String) -> Result<String, ()> {
  for attempt in 1..3 {
    println!("[INF] Requesting {:?}", nyaa_url);
    let get_request = isahc::Request::get(nyaa_url)
      .timeout(Duration::from_secs(15))
      .body(()).expect("Failed to create request.")
    .send();
  
    thread::sleep(Duration::from_secs(2));
    
    if let Ok(mut request) = get_request {
      if request.status() == StatusCode::OK {
        match request.text() {
          Ok(nya) => {
            return Ok(nya);
          },
          Err(e) => {
            eprintln!("Failed nyaa request:\n{:?}", e);
            return Err(());
          }
        }
      }
    }
    eprintln!("Failed to send get request (attempt: {})", attempt);
  }

  eprintln!("Skipping request ...");
  Err(())
}

fn get_domain(url: &str) -> String {
  let re = regex::Regex::new(r"https?://([a-zA-Z]+.[a-z]+|[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3})/").unwrap();
  return re.find(url).unwrap().as_str().to_string()
}
