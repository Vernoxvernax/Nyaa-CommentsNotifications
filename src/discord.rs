use std::{
  sync::Arc, thread, time::Duration
};
use chrono::{
  DateTime, Utc, NaiveDateTime
};
use serenity::{
  all::{
    ActivityData, ButtonStyle, Command, CreateButton, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, Interaction
  },
  prelude::{
    EventHandler, Context, Mentionable
  },
  model::{
    prelude::{
      Ready, ChannelId, ReactionType, RoleId
    },
    Color
  },
  builder::CreateEmbed,
  http::Http,
  async_trait
};
use sqlx::{
  Pool, Sqlite
};

use crate::web::{
  NyaaUpdate, NyaaCommentUpdateType, NyaaComment
};
use crate::config::ModuleConfig;
use crate::commands;

pub struct Handler {
  pub database_pool: Pool<Sqlite>,
  pub discord_bot_id: String,
  pub discord_activity_type: String,
  pub discord_activity_text: String
}

#[async_trait]
impl EventHandler for Handler {
  async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
    if let Interaction::Command(command) = interaction {
      // println!("Received command interaction: {:#?}", command); // uncomment if you want to monitor all commands being answered by the bot
      let content = match command.data.name.as_str() {
        "help" => commands::help::run(&command.data.options).await,
        "create" => commands::create::run(&command.data.options, &self.discord_bot_id, self.database_pool.clone()).await,
        "reset" => commands::reset::run(&command.data.options, &self.discord_bot_id, self.database_pool.clone()).await,
        "pause" => commands::pause::run(&command.data.options, &self.discord_bot_id, self.database_pool.clone()).await,
        "activity" => commands::activity::run(&command.data.options, &ctx).await,
        _ => "Not implemented >~< - (Contact: @DepriSheep)".to_string(),
      };
      
      let data = CreateInteractionResponseMessage::new().content(content);
      let builder = CreateInteractionResponse::Message(data);
      if let Err(why) = command.create_response(&ctx.http, builder).await {
        println!("Cannot respond to slash command: {}", why);
      }
    }
  }

  async fn ready(&self, ctx: Context, ready: Ready) {
    println!("[Discord] {} is conntected.", ready.user.name);
    if self.discord_activity_type == "listening" {
      ctx.set_activity(Some(ActivityData::listening(&self.discord_activity_text)));
    } else if self.discord_activity_type == "playing" {
      ctx.set_activity(Some(ActivityData::playing(&self.discord_activity_text)));
    } else if self.discord_activity_type == "watching" {
      ctx.set_activity(Some(ActivityData::watching(&self.discord_activity_text)));
    } else if self.discord_activity_type == "competing" {
      ctx.set_activity(Some(ActivityData::competing(&self.discord_activity_text)));
    } else {
      eprintln!("Activity type not found. Options are: \"playing; watching; competing; listening\".");
    }
    
    Command::create_global_command(&ctx.http, commands::help::register()).await.unwrap();
    Command::create_global_command(&ctx.http, commands::create::register()).await.unwrap();
    Command::create_global_command(&ctx.http, commands::reset::register()).await.unwrap();
    Command::create_global_command(&ctx.http, commands::pause::register()).await.unwrap();
    Command::create_global_command(&ctx.http, commands::activity::register()).await.unwrap();
  }
}

pub async fn discord_send_updates(http: Arc<Http>, module: &ModuleConfig, updates: Vec<NyaaUpdate>) -> Result<Vec<NyaaUpdate>, ()> {
  let mut successful_updates: Vec<NyaaUpdate> = vec![];
  let channel = ChannelId::new(module.discord_channel_id.unwrap());
  for attempt in 1..5 {
    if channel.to_channel(&http).await.is_err() {
      if attempt == 5 {
        println!("[INF] Channel \"{:?}\" is unreachable.\nPausing notifications.", channel.get());
        return Err(());
      }
      thread::sleep(Duration::from_secs(3));
    } else {
      break;
    }
  }
  for update in updates {
    let title = limit_string_length(&update.torrent.title, 100);
    let mut only_upload = update.torrent.clone();
    only_upload.comments = vec![];
    if update.new_upload && module.uploads.unwrap() {
      let utc_time = unix_to_datetime(update.torrent.upload_date_timestamp);
      if let Ok(()) = send_discord_embed(&http,
        channel, module.discord_pinged_role, &title,
        update.torrent.uploader.clone().unwrap().avatar.unwrap(),
        vec![("Category".to_string(), update.torrent.category.clone(), true), ("Size".to_string(), update.torrent.size.clone(), true)],
        utc_time,
        ("Nyaa.si".to_string(), "Torrent-File".to_string()),
        (
          format!("{}view/{}", update.torrent.domain, update.torrent.id),
          format!("{}download/{}.torrent", update.torrent.domain, update.torrent.id)
        ),
        (ReactionType::Unicode("📰".to_string()), ReactionType::Unicode("📁".to_string()))
      ).await {
        successful_updates.append(&mut vec![NyaaUpdate {
          new_upload: true,
          torrent: only_upload
        }]);
      } else {
        continue;
      };
    } else if update.new_upload && ! module.uploads.unwrap() {
      successful_updates.append(&mut vec![NyaaUpdate {
        new_upload: true,
        torrent: only_upload
      }]);
    }

    let mut only_comment_updates = update.torrent.clone();
    only_comment_updates.comments = vec![];
    if !update.torrent.comments.is_empty() && module.comments.unwrap() {
      for comment in update.torrent.comments {
        thread::sleep(Duration::from_secs(1));
        match comment.update_type {
          NyaaCommentUpdateType::DELETED => {
            if let Err(()) = send_discord_embed(&http,
              channel, module.discord_pinged_role, &title,
              comment.user.avatar.clone().unwrap(),
              vec![(comment.user.username.clone()+" (deleted comment)", comment.message.clone(), false)],
              chrono::offset::Utc::now(),
              ("Nyaa.si".to_string(), comment.user.username.clone()),
              (
                format!("{}view/{}", update.torrent.domain, update.torrent.id),
                format!("{}user/{}", update.torrent.domain, comment.user.username.clone())
              ),
              (ReactionType::Unicode("💬".to_string()), ReactionType::Unicode("🕵️".to_string()))
            ).await {
              only_comment_updates.comments.append(&mut vec![NyaaComment {
                user: comment.user,
                message: comment.message,
                old_message: comment.old_message,
                uploader: comment.uploader,
                date_timestamp: comment.date_timestamp,
                edited_timestamp: comment.edited_timestamp,
                old_edited_timestamp: comment.old_edited_timestamp,
                direct_link: comment.direct_link,
                update_type: NyaaCommentUpdateType::UNCHECKED
              }]);
            };
          },
          NyaaCommentUpdateType::EDITED => {
            let utc_time = unix_to_datetime(comment.edited_timestamp.unwrap());
            if let Ok(()) = send_discord_embed(&http,
              channel, module.discord_pinged_role, &title,
              comment.user.avatar.clone().unwrap(),
              vec![
                (comment.user.username.clone()+" (old comment)", "```".to_owned()+&comment.old_message.clone().unwrap()+"```", true),
                ("new:".to_string(), "```".to_owned()+&comment.message.clone()+"```", true)
              ],
              utc_time,
              ("Comment@Nyaa.si".to_string(), comment.user.username.clone()),
              (
                comment.direct_link.clone(),
                format!("{}user/{}", update.torrent.domain, comment.user.username.clone())
              ),
              (ReactionType::Unicode("💬".to_string()), ReactionType::Unicode("🕵️".to_string()))
            ).await {
              only_comment_updates.comments.append(&mut vec![comment]);
            } else {
              only_comment_updates.comments.append(&mut vec![NyaaComment {
                user: comment.user,
                message: comment.old_message.unwrap(),
                old_message: None,
                uploader: comment.uploader,
                date_timestamp: comment.date_timestamp,
                edited_timestamp: comment.old_edited_timestamp,
                old_edited_timestamp: None,
                direct_link: comment.direct_link,
                update_type: NyaaCommentUpdateType::UNCHECKED
              }]);
            };
          },
          NyaaCommentUpdateType::NEW => {
            let utc_time = unix_to_datetime(comment.date_timestamp);
            if let Ok(()) = send_discord_embed(&http,
              channel, module.discord_pinged_role, &title,
              comment.user.avatar.clone().unwrap(),
              vec![(comment.user.username.clone(), comment.message.clone(), false)],
              utc_time,
              ("Comment@Nyaa.si".to_string(), comment.user.username.clone()),
              (
                comment.direct_link.clone(),
                format!("{}user/{}", update.torrent.domain, comment.user.username.clone())
              ),
              (ReactionType::Unicode("💬".to_string()), ReactionType::Unicode("🕵️".to_string()))
            ).await {
              let mut finished_comment = comment.clone();
              finished_comment.update_type = NyaaCommentUpdateType::UNCHECKED;
              only_comment_updates.comments.append(&mut vec![finished_comment]);
            };
          },
          NyaaCommentUpdateType::UNDECIDED | NyaaCommentUpdateType::UNCHECKED => {
            only_comment_updates.comments.append(&mut vec![comment]);
          }
        }
      }
      successful_updates.append(&mut vec![NyaaUpdate {
        new_upload: false,
        torrent: only_comment_updates
      }]);
    } else if ! module.comments.unwrap() {
      for comment in update.torrent.comments.clone() {
        if comment.update_type == NyaaCommentUpdateType::DELETED {
          continue;
        }
        only_comment_updates.comments.append(&mut vec![comment]);
      }
      successful_updates.append(&mut vec![NyaaUpdate {
        new_upload: false,
        torrent: only_comment_updates
      }]);
    }
    thread::sleep(Duration::from_secs(1));
  }
  Ok(successful_updates)
}

pub fn unix_to_datetime(timestamp: f64) -> DateTime<Utc> {
  let naive_datetime = NaiveDateTime::from_timestamp_opt(timestamp as i64, 0).unwrap();
  DateTime::from_naive_utc_and_offset(naive_datetime, Utc)
}

pub fn limit_string_length(input: &str, limit: usize) -> String {
  let mut split = String::new();
  for (index, char) in input.char_indices() {
    if index == limit {
      split += "...";
      break;
    } else {
      split.push(char);
    }
  }
  split
}

async fn send_discord_embed(http: &Arc<Http>, channel: ChannelId, discord_pinged_role: Option<u64>, title: &str, thumbnail: String, fields: Vec<(String, String, bool)>,
utc_time: DateTime<Utc>, button_labels: (String, String), button_urls: (String, String), button_emojis: (ReactionType, ReactionType)) -> Result<(), ()> {
  for field in create_embeds_after_size(fields) {
    let mut embed: CreateEmbed = serenity::builder::CreateEmbed::default();
    embed = embed
      .title(title)
      .color(Color::BLITZ_BLUE)
      .thumbnail(thumbnail.clone())
      .fields(field)
    .timestamp(utc_time);

    let buttons: Vec<CreateButton> = vec![
      CreateButton::new_link(button_urls.0.clone())
      .label(button_labels.0.clone())
      .style(ButtonStyle::Primary)
      .emoji(button_emojis.0.clone()),
      CreateButton::new_link(button_urls.1.clone())
      .label(button_labels.1.clone())
      .style(ButtonStyle::Primary)
      .emoji(button_emojis.1.clone()),
    ];

    let role_id = discord_pinged_role.unwrap();
    if role_id != 0 {
      if let Err(e) = channel.send_message(&http,
        CreateMessage::new()
          .content(RoleId::new(role_id).mention().to_string())
          .embed(
            embed
          )
          .button(
            buttons.get(0).unwrap().clone()
          )
          .button(
            buttons.get(1).unwrap().clone()
          )
      ).await {
        eprintln!("Error sending message: {:?}", e);
        return Err(());
      }
    } else if let Err(e) = channel.send_message(&http,
      CreateMessage::new()
        .embed(
          embed
        )
        .button(
          buttons.get(0).unwrap().clone()
        )
        .button(
          buttons.get(1).unwrap().clone()
        )
    ).await {
      eprintln!("Error sending message: {:?}", e);
      return Err(());
    }
  }
  Ok(())
}

fn create_embeds_after_size(fields: Vec<(String, String, bool)>) -> Vec<Vec<(String, String, bool)>> {
  let mut max_size = 1010; 
  let mut output: Vec<Vec<(String, String, bool)>> = vec![];
  let single_size = calculate_single_size(fields.clone());
  let mut fields_copy = fields.clone();
  if single_size > max_size {
    let total_parts = calculate_total_parts(fields.clone(), max_size);
    let mut message_index = 1;
    let mut temp: (String, String, bool) = ("1".to_string(), "1".to_string(), false);
    let mut field1_done = false;
    let mut field2_done = false;
    loop {
      for (index, (name, text, inline)) in fields_copy.iter_mut().enumerate() {
        if message_index > 10 {
          break;
        } 
        if (fields.len() == 1) && (message_index != 1) {
          output.append(&mut vec![
            vec![temp.clone()]
          ]);
        }

        let mut name = name.clone();
        if index == 0 {
          name += format!(" ({}/{}):", message_index, total_parts).as_str();
        }

        let message: String;
        if fields.len() == 1 {
          if max_size < (text.len() + name.len()) {
            message = text.clone().split_at(max_size-name.len()).0.to_string();
            *text = text.clone().split_at(max_size-name.len()).1.to_string();
          } else {
            message = text.clone();
            output.append(&mut vec![
              vec![(name, message, *inline)]
            ]);
            return output;
          }
        } else if index == 0 {
          if (name.len() + text.len() + temp.0.len()) > (max_size / 2) {
            message = text.clone().split_at((max_size/2)-(name.len()+temp.0.len())).0.to_string();
            *text = text.clone().split_at((max_size/2)-(name.len()+temp.0.len())).1.to_string();
          } else if (name.len() + text.len() + temp.0.len()) > (max_size) && (temp.1.is_empty()) {
            message = text.clone().split_at((max_size)-(name.len()+temp.0.len())).0.to_string();
            *text = text.clone().split_at((max_size)-(name.len()+temp.0.len())).1.to_string();
          } else {
            message = text.clone();
            *text = "".to_string();
            field1_done = true;
          }
        } else if (name.len() + text.len()) > (max_size / 2) && (!temp.1.is_empty()) {
          message = text.clone().split_at((max_size/2)-(name.len()-1)).0.to_string();
          *text = text.clone().split_at((max_size/2)-name.len()-1).1.to_string();
        } else if (name.len() + text.len() + temp.0.len()) > (max_size) && (temp.1.is_empty()) {
          message = text.clone().split_at((max_size)-(name.len()+temp.0.len())).0.to_string();
          *text = text.clone().split_at((max_size)-(name.len()+temp.0.len())).1.to_string();
        } else {
          message = text.clone();
          *text = "".to_string();
          max_size *= 2;
          field2_done = true;
        }

        if index == 0 {
          temp = (name, message.clone(), *inline);
          if field1_done && ((message_index > 1) && temp.1 == "1") {
            return output;
          }
          message_index += 1;
        } else if index == 1 {
          output.append(&mut vec![
            vec![temp.clone(), (name.clone(), message.to_string(), *inline)]
          ]);
          temp = (name, message.clone(), *inline);
          if field1_done && field2_done {
            return output;
          }
        }
      }
    }
  } else {
    for (index, field) in fields_copy.iter_mut().enumerate() {
      if index == 0 {
        field.0.push(':');
      }
    }
    vec![fields_copy]
  }
}

fn calculate_single_size(fields: Vec<(String, String, bool)>) -> usize {
  let mut size = 1;
  for (name, value, _) in fields {
    size += name.len() + value.len();
  }
  size
}

fn calculate_total_parts(fields: Vec<(String, String, bool)>, max_size: usize) -> usize {
  let mut amount = 0;
  let field1_name = fields.get(0).unwrap().0.len();
  let mut field1_message = fields.get(0).unwrap().1.len();
  if fields.len() > 1 {
      let field2_name = fields.get(1).unwrap().0.len();
      let mut field2_message = fields.get(1).unwrap().1.len();
      while !((field1_message == 0) && (field2_message == 0)) {
        let mut no_change = false;
        amount += 1;
        if (field1_message > (max_size / 2) - (field1_name + field2_name + 6)) && 
        (field2_message != 0) {
          field1_message -= (max_size / 2) - (field1_name + field2_name + 6);
        } else if (field1_message > (max_size) - (field1_name + field2_name + 6)) && 
        (field2_message == 0) {
          field1_message -= (max_size) - ((field1_name + 6) + field2_name);
        } else {
          field1_message = 0;
          no_change = true;
        }
        
        if field2_message > (max_size / 2) - field2_name {
          field2_message -= max_size / 2 - (field2_name);
        } else if (field2_message > (max_size) - (field2_name + field1_name + 6)) && 
        (field1_message == 0) {
          field2_message -= (max_size) - (field1_name + field2_name + 6);
        } else if no_change {
          break;
        } else {
          field2_message = 0;
        }
      }
  } else {
      while field1_message != 0 {
        amount += 1;
        if field1_message > (max_size) - (field1_name + 6) {
          field1_message -= (max_size) - (field1_name + 6);
        } else {
          break;
        }
      }
  }
  amount
}
