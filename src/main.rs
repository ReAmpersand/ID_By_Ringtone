use std::{env, sync::Arc, fs::File, io::Write};

use serenity::client::bridge::voice::ClientVoiceManager;

use serenity::{client::Context, prelude::Mutex};
use serenity::model::id::GuildId;
use serenity::model::voice::VoiceState;

use serenity::{
    client::{Client, EventHandler},
    framework::{
        StandardFramework,
        standard::{
            Args, CommandResult,
            macros::{command, group},
        },
    },
    model::{channel::Message, gateway::Ready, misc::Mentionable},
    Result as SerenityResult,
    voice,
};

// This imports `typemap`'s `Key` as `TypeMapKey`.
use serenity::prelude::*;

struct VoiceManager;

impl TypeMapKey for VoiceManager {
    type Value = Arc<Mutex<ClientVoiceManager>>;
}

struct Handler;

impl EventHandler for Handler {
    fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
    fn voice_state_update(&self, ctx: Context, guild_id:Option<GuildId>, old: Option<VoiceState>, new: VoiceState) {
        let cache = ctx.cache.read();
        let member = cache.member(guild_id.unwrap(), new.user_id);
        let user_arc;
        let user;

        match member {
            Some(member) => {
                user_arc = Arc::clone(&member.user);
                user = user_arc.read();
            },
            None => {
                println!("None Member");
                return;
            },
        }

        if let Some(_old) = old {
            return;
        }

        match new.channel_id {
            Some(channel_id) => {
                // Play Ringtone.
                let manager_lock = ctx.data.read().get::<VoiceManager>().cloned().expect("Expected VoiceManager in ShareMap.");
                let mut manager = manager_lock.lock();

                if manager.join(guild_id.unwrap(), channel_id).is_some() {
                    if let Some(handler) = manager.get_mut(guild_id.unwrap()) {
                        let ext: &str = ".mp3";
                        let folder: &str = "ringtone/";
                        let source = match voice::ffmpeg(format!("{}{}{}", folder, user.name, ext)) {
                            Ok(source) => source,
                            Err(why) => {
                                println!("Err starting source: {:?}", why);
                                return;
                            },
                        };
                        handler.play(source);
                    }
                }
            },
            None => (),
        }
    }
}

group!({
    name: "general",
    options: {},
    commands: [join, leave, mute, play, ping, unmute, upload]
});

fn main() {
    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN")
        .expect("Expected a token in the environment");
    let mut client = Client::new(&token, Handler).expect("Err creating client");

    // Obtain a lock to the data owned by the client, and insert the client's
    // voice manager into it. This allows the voice manager to be accessible by
    // event handlers and framework commands.
    {
        let mut data = client.data.write();
        data.insert::<VoiceManager>(Arc::clone(&client.voice_manager));
    }

    client.with_framework(StandardFramework::new()
        .configure(|c| c
            .prefix("~"))
        .group(&GENERAL_GROUP));

    let _ = client.start().map_err(|why| println!("Client ended: {:?}", why));
}

#[command]
fn join(ctx: &mut Context, msg: &Message) -> CommandResult {
    let guild = match msg.guild(&ctx.cache) {
        Some(guild) => guild,
        None => {
            check_msg(msg.channel_id.say(&ctx.http, "Groups and DMs not supported"));

            return Ok(());
        }
    };

    let guild_id = guild.read().id;

    let channel_id = guild
        .read()
        .voice_states.get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);


    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            check_msg(msg.reply(&ctx, "Not in a voice channel"));

            return Ok(());
        }
    };

    let manager_lock = ctx.data.read().get::<VoiceManager>().cloned().expect("Expected VoiceManager in ShareMap.");
    let mut manager = manager_lock.lock();

    if manager.join(guild_id, connect_to).is_some() {
        check_msg(msg.channel_id.say(&ctx.http, &format!("Joined {}", connect_to.mention())));
    } else {
        check_msg(msg.channel_id.say(&ctx.http, "Error joining the channel"));
    }

    Ok(())
}

#[command]
fn leave(ctx: &mut Context, msg: &Message) -> CommandResult {
    let guild_id = match ctx.cache.read().guild_channel(msg.channel_id) {
        Some(channel) => channel.read().guild_id,
        None => {
            check_msg(msg.channel_id.say(&ctx.http, "Groups and DMs not supported"));

            return Ok(());
        },
    };

    let manager_lock = ctx.data.read().get::<VoiceManager>().cloned().expect("Expected VoiceManager in ShareMap.");
    let mut manager = manager_lock.lock();
    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        manager.remove(guild_id);

        check_msg(msg.channel_id.say(&ctx.http, "Left voice channel"));
    } else {
        check_msg(msg.reply(&ctx, "Not in a voice channel"));
    }

    Ok(())
}

#[command]
fn mute(ctx: &mut Context, msg: &Message) -> CommandResult {
    let guild_id = match ctx.cache.read().guild_channel(msg.channel_id) {
        Some(channel) => channel.read().guild_id,
        None => {
            check_msg(msg.channel_id.say(&ctx.http, "Groups and DMs not supported"));

            return Ok(());
        },
    };

    let manager_lock = ctx.data.read().get::<VoiceManager>().cloned().expect("Expected VoiceManager in ShareMap.");
    let mut manager = manager_lock.lock();

    let handler = match manager.get_mut(guild_id) {
        Some(handler) => handler,
        None => {
            check_msg(msg.reply(&ctx, "Not in a voice channel"));

            return Ok(());
        },
    };

    if handler.self_mute {
        check_msg(msg.channel_id.say(&ctx.http, "Already muted"));
    } else {
        handler.mute(true);

        check_msg(msg.channel_id.say(&ctx.http, "Now muted"));
    }

    Ok(())
}

#[command]
fn ping(context: &mut Context, msg: &Message) -> CommandResult {
    check_msg(msg.channel_id.say(&context.http, "Pong!"));

    Ok(())
}

#[command]
fn play(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let url = match args.single::<String>() {
        Ok(url) => url,
        Err(_) => {
            check_msg(msg.channel_id.say(&ctx.http, "Must provide a URL to a video or audio"));

            return Ok(());
        },
    };

    if !url.starts_with("http") {
        check_msg(msg.channel_id.say(&ctx.http, "Must provide a valid URL"));

        return Ok(());
    }

    let guild_id = match ctx.cache.read().guild_channel(msg.channel_id) {
        Some(channel) => channel.read().guild_id,
        None => {
            check_msg(msg.channel_id.say(&ctx.http, "Error finding channel info"));

            return Ok(());
        },
    };

    let manager_lock = ctx.data.read().get::<VoiceManager>().cloned().expect("Expected VoiceManager in ShareMap.");
    let mut manager = manager_lock.lock();

    if let Some(handler) = manager.get_mut(guild_id) {
        let source = match voice::ytdl(&url) {
            Ok(source) => source,
            Err(why) => {
                println!("Err starting source: {:?}", why);

                check_msg(msg.channel_id.say(&ctx.http, "Error sourcing ffmpeg"));

                return Ok(());
            },
        };

        handler.play(source);

        check_msg(msg.channel_id.say(&ctx.http, "Playing song"));
    } else {
        check_msg(msg.channel_id.say(&ctx.http, "Not in a voice channel to play in"));
    }

    Ok(())
}

#[command]
fn unmute(ctx: &mut Context, msg: &Message) -> CommandResult {
    let guild_id = match ctx.cache.read().guild_channel(msg.channel_id) {
        Some(channel) => channel.read().guild_id,
        None => {
            check_msg(msg.channel_id.say(&ctx.http, "Error finding channel info"));

            return Ok(());
        },
    };
    let manager_lock = ctx.data.read().get::<VoiceManager>().cloned().expect("Expected VoiceManager in ShareMap.");
    let mut manager = manager_lock.lock();

    if let Some(handler) = manager.get_mut(guild_id) {
        handler.mute(false);

        check_msg(msg.channel_id.say(&ctx.http, "Unmuted"));
    } else {
        check_msg(msg.channel_id.say(&ctx.http, "Not in a voice channel to unmute in"));
    }

    Ok(())
}

#[command]
fn upload(context: &mut Context, message: &Message) -> CommandResult {
    let folder: &str = "ringtone/";
    let ext: &str = ".mp3";

    for attachment in &message.attachments {
        let content = match attachment.download() {
            Ok(content) => content,
            Err(why) => {
                println!("Error downloading attachment: {:?}", why);
                let _ = message.channel_id.say(&context.http, "Error downloading attachment");

                return Ok(());
            },
        };

        let mut file = match File::create(format!("{}{}{}", folder, message.author.name, ext)) {
            Ok(file) => file,
            Err(why) => {
                println!("Error creating file: {:?}", why);
                let _ = message.channel_id.say(&context.http, "Error creating file");

                return Ok(());
            },
        };

        if let Err(why) = file.write(&content) {
            println!("Error writing to file: {:?}", why);

            return Ok(());
        }

        let _ = message.channel_id.say(&context.http, &format!("Saved {:?}", message.author.name));
    }

    Ok(())
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}
