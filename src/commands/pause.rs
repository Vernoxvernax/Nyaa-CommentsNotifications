use serenity::all::{
  ChannelType, Permissions, CreateCommand, CreateCommandOption, CommandOptionType, CommandDataOptionValue, CommandDataOption
};
use sqlx::{
  Sqlite, Pool
};

use crate::database::Database;

pub async fn run(options: &[CommandDataOption], discord_bot_id: &String, database_pool: Pool<Sqlite>) -> String {
  let channel_id = match options.get(0).unwrap().value {
    CommandDataOptionValue::Channel(integer) => integer.get(),
    _ => {
      panic!("Discord returned invalid command options.")
    }
  };

  let mut database: Database;
  if let Ok(database_) = Database::use_pool(database_pool).await {
    database = database_;
  } else {
    return "Failed to connect to database".to_string();
  }

  let check = database.get_discord_channels(discord_bot_id).await;
  if check.is_empty() {
    return "This discord channel has not been configured yet. Type `/create` to set it up.".to_string();
  }
  let mode = !check.iter().find(|module| module.discord_channel_id.unwrap() == channel_id).unwrap().active;

  database.pause_discord_channel(discord_bot_id, channel_id, mode).await;
  if !mode {
    "Successfully paused your channel.".to_string()
  } else {
    "Successfully unpaused your channel.".to_string()
  }
}

pub fn register() -> CreateCommand {
  CreateCommand::new("pause")
    .description("Toggle the notification-state for the current channel")
    .default_member_permissions(Permissions::ADMINISTRATOR)
    .add_option(CreateCommandOption::new(
      CommandOptionType::Channel,
      "channel",
      "Channel to receive the notifications"
      )
      .channel_types([ChannelType::Text].to_vec())
      .required(true)
  )
}
