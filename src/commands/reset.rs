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

  if ! database.discord_channel_exists(discord_bot_id, channel_id).await {
    return "This discord channel has never been configured. See `/create`.".to_string();
  }

  database.remove_discord_channel(discord_bot_id, channel_id).await;

	println!("Configuration removed for {:?}", channel_id);
	"Channel configuration successfully removed.".to_string()
}

pub fn register() -> CreateCommand {
	CreateCommand::new("reset")
		.description("Remove configurations for the current channel")
		.add_option(
      CreateCommandOption::new(
        CommandOptionType::Channel,
        "channel",
        "Channel that had received notifications"
      )
      .channel_types([ChannelType::Text].to_vec())
      .required(true)
    )
		.default_member_permissions(Permissions::ADMINISTRATOR)
}
