use serenity::all::{
  Permissions, CreateCommand, CreateCommandOption, CommandOptionType, CommandDataOptionValue, CommandDataOption, ChannelType
};
use sqlx::{
  Pool, Sqlite
};

use crate::database::Database;

pub async fn run(options: &[CommandDataOption], discord_bot_id: &String, database_pool: Pool<Sqlite>) -> String {
  let channel_id = match options.get(0).unwrap().value {
    CommandDataOptionValue::Channel(integer) => integer.get(),
    _ => {
      panic!("Discord returned invalid command options.")
    }
  };
  let url_input = match &options.get(1).unwrap().value {
    CommandDataOptionValue::String(text) => text,
    _ => {
      panic!("Discord returned invalid command options.")
    }
  };
  let uploads = match options.get(2).unwrap().value {
    CommandDataOptionValue::Boolean(boolean) => boolean,
    _ => {
      panic!("Discord returned invalid command options.")
    }
  };
  let comments = match options.get(3).unwrap().value {
    CommandDataOptionValue::Boolean(boolean) => boolean,
    _ => {
      panic!("Discord returned invalid command options.")
    }
  };
  let complete = match options.get(4).unwrap().value {
    CommandDataOptionValue::Boolean(boolean) => boolean,
    _ => {
      panic!("Discord returned invalid command options.")
    }
  };
  let pinged_role: String = match &options.get(5) {
    Some(arg) => {
      match &arg.value {
        CommandDataOptionValue::Role(role) => role.get().to_string(),
        _ => {
          panic!("Discord returned invalid command options.")
        }
      }
    },
    None => {
      "0".to_string()
    }
  };

  let mut database: Database;
  if let Ok(database_) = Database::use_pool(database_pool).await {
    database = database_;
  } else {
    return "Failed to connect to database".to_string();
  }

  if database.discord_channel_exists(discord_bot_id, channel_id).await {
    return "This discord channel has already been configured, please make sure to `/reset` it before creating new settings.".to_string();
  }
  
  let urls: Vec<String> = url_input.split(',').map(|str| str.trim().to_string()).collect();

  println!("[INF] {:?} configured with {:?} | {} {} {}", channel_id, urls, uploads, comments, complete);
  
  database.add_discord_channel(discord_bot_id, channel_id, urls, (comments, uploads, complete), pinged_role.clone()).await;
  "Channel successfully configured.".to_string()
}

pub fn register() -> CreateCommand {
  CreateCommand::new("create")
    .description("Setup notifications for a channel")
    .add_option(
      CreateCommandOption::new(
        CommandOptionType::Channel,
        "channel",
        "Channel to receive the notifications"
      )
      .channel_types([ChannelType::Text].to_vec())
      .required(true)
    )
    .add_option(
      CreateCommandOption::new(
        CommandOptionType::String,
        "url",
        "Nyaa URL separated by `,` (f.e.: `https://nyaa.si/user/neoborn, https://nyaa.si/user/djatom`)"
      )
      .min_length(5)
      .required(true)
    )
    .add_option(
      CreateCommandOption::new(
        CommandOptionType::Boolean,
        "uploads",
        "Notifications for uploads"
      )
      .required(true)
    )
    .add_option(
      CreateCommandOption::new(
        CommandOptionType::Boolean,
        "comments",
        "Notifications for comments",
      )
      .kind(CommandOptionType::Boolean)
      .required(true)
    )
    .add_option(
      CreateCommandOption::new(
        CommandOptionType::Boolean,
        "complete",
        "Check every page of the query, or all of them?"
      )
      .required(true)
    )
    .add_option(
      CreateCommandOption::new(
        CommandOptionType::Role,
        "pinged-role",
        "Ping this role when sending the notifications"
      )
      .required(false)
    )
    .default_member_permissions(Permissions::ADMINISTRATOR)
}
