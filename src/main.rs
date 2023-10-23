extern crate discord;
extern crate rand;
extern crate sqlite;

use discord::{
    model::{Event, ReactionEmoji},
    Discord,
};
use rand::{prelude::*, thread_rng};
use sqlite::Row;
use std::env;

struct Question {
    question: String,
    answer: String,
    score: i64,
}

impl Question {
    fn new(question: String, answer: String, score: i64) -> Self {
        Question {
            question,
            answer,
            score,
        }
    }
}

fn next_question(r: Row) -> Question {
    Question::new(
        r.read::<&str, _>("interpretation").to_string(),
        r.read::<&str, _>("word").replace(|c: char| c == '\"', ""),
        r.read::<i64, _>("id_syn"),
    )
}

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
    let mut current_question = next_question(data.pop().unwrap());

    loop {
        match connection.recv_event() {
            Ok(Event::MessageCreate(message)) => {
                let text = message.content.to_owned();
                println!(
                    "{}: {} says: {}",
                    message.timestamp, message.author.name, text
                );
                if text == "!test" {
                    if message.author.name != "Word Game Bot" {
                        let _ = discord.send_message(
                            message.channel_id,
                            format!("This is a reply to the message. {}", text).as_str(),
                            "",
                            false,
                        );
                    }
                } else if text == "!next" || text == "!далі" {
                    current_question = next_question(data.pop().unwrap());
                    let _ = discord.send_message(
                        message.channel_id,
                        &current_question.question,
                        "",
                        false,
                    );
                } else if text == "!відповідь" {
                    let _ = discord.send_message(
                        message.channel_id,
                        &current_question.answer,
                        "",
                        false,
                    );
                    current_question = next_question(data.pop().unwrap());
                    let _ = discord.send_message(
                        message.channel_id,
                        &current_question.question,
                        "",
                        false,
                    );
                } else if text.trim().to_lowercase() == "!q" {
                    let _ = discord.send_message(
                        message.channel_id,
                        &current_question.question,
                        "",
                        false,
                    );
                } else if text.trim().to_lowercase() == "!підказка" {
                    let _ = discord.send_message(
                        message.channel_id,
                        &current_question
                            .answer
                            .chars()
                            .into_iter()
                            .rev()
                            .last()
                            .unwrap()
                            .to_string(),
                        "",
                        false,
                    );
                } else if text.to_lowercase().trim() == current_question.answer {
                    // ansver verify and TODO: set score
                    let _ = discord.send_message(
                        message.channel_id,
                        format!(
                            "Вірно {}. Відповідь {}. Рейтинг: {}",
                            message.author.mention(),
                            current_question.answer,
                            current_question.score //TODO
                        )
                        .as_str(),
                        "",
                        false,
                    );
                    current_question = next_question(data.pop().unwrap());
                    let _ = discord.send_message(
                        message.channel_id,
                        &current_question.question,
                        "",
                        false,
                    );
                } else if !message.author.bot {
                    let _ = discord.add_reaction(
                        message.channel_id,
                        message.id,
                        ReactionEmoji::Unicode("➖".to_string()),
                    );
                }
                //} else if text == "!quit" {
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
