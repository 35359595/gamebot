extern crate discord;
extern crate rand;
extern crate sqlite;

use discord::model::Event;
use discord::Discord;
use rand::{prelude::*, thread_rng};
use sqlite::Row;
use std::env;

fn main() {
    println!("Opening DB");
    let mut db_path = env::var("CARGO_MANIFEST_DIR").expect("No manifest dir");
    db_path.push_str("/db/synsets_ua.db");
    println!("DB path: {}", &db_path);

    // Verb selector
    let query = "SELECT * FROM wlist WHERE interpretation IS NOT NULL";
    let db = sqlite::open(&db_path).expect("db expected");
    let mut data: Vec<Row> = db
        .prepare(query)
        .unwrap()
        .into_iter()
        .map(|row| row.unwrap())
        .collect();

    // RNG
    let mut rng = thread_rng();
    data.shuffle(&mut rng);

    // Log in to Discord using a bot token from the environment
    let discord = Discord::from_bot_token(&env::var("DISCORD_TOKEN").expect("Expected token"))
        .expect("login failed");

    // Establish and use a websocket connection
    let (mut connection, _) = discord.connect().expect("connect failed");
    println!("Ready.");
    let mut current_answer = String::default();
    let mut current_question = String::default();
    let mut current_weight = 0;

    loop {
        match connection.recv_event() {
            Ok(Event::MessageCreate(message)) => {
                println!(
                    "{}: {} says: {}",
                    message.timestamp, message.author.name, message.content
                );
                if message.content == "!test" {
                    if message.author.name != "Word Game Bot" {
                        let _ = discord.send_message(
                            message.channel_id,
                            format!("This is a reply to the message. {}", message.content).as_str(),
                            "",
                            false,
                        );
                    }
                } else if message.content == "!next" || message.content == "!далі" {
                    let new_question = data.pop().unwrap();
                    current_question = new_question.read::<&str, _>("interpretation").to_string();
                    current_answer = new_question
                        .read::<&str, _>("word")
                        .replace(|c: char| c == '\"', "");
                    current_weight = new_question.read::<i64, _>("id_syn");
                    let _ = discord.send_message(message.channel_id, &current_question, "", false);
                } else if message.content.trim().to_lowercase() == "!q" {
                    let _ = discord.send_message(message.channel_id, &current_question, "", false);
                } else if message.content.trim().to_lowercase() == "!підказка" {
                    let _ = discord.send_message(
                        message.channel_id,
                        &current_answer
                            .chars()
                            .into_iter()
                            .rev()
                            .last()
                            .unwrap()
                            .to_string(),
                        "",
                        false,
                    );
                } else if message.content.to_lowercase().trim() == current_answer {
                    // ansver verify and TODO: set score
                    let _ = discord.send_message(
                        message.channel_id,
                        format!(
                            "Вірно {}. Відповідь {}. Рейтинг: {}",
                            message.author.mention(),
                            current_answer,
                            current_weight //TODO
                        )
                        .as_str(),
                        "",
                        false,
                    );
                    let new_question = data.pop().unwrap();
                    current_question = new_question.read::<&str, _>("interpretation").to_string();
                    current_answer = new_question
                        .read::<&str, _>("word")
                        .replace(|c: char| c == '\"', "");
                    let _ = discord.send_message(message.channel_id, &current_question, "", false);
                }
                //} else if message.content == "!quit" {
                //    println!("Quitting.");
                //    break;
                //}
            }
            Ok(_) => {}
            Err(discord::Error::Closed(code, body)) => {
                println!("Gateway closed on us with code {:?}: {}", code, body);
                break;
            }
            Err(err) => println!("Receive error: {:?}", err),
        }
    }
}
