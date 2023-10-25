extern crate discord;
extern crate rand;
extern crate regex;
extern crate sqlite;

use discord::{
    model::{Event, ReactionEmoji},
    Discord,
};
use rand::{prelude::*, thread_rng};
use regex::Regex;
use sqlite::{Connection, Row};
use std::{env, fmt::Display};

struct Question {
    question: String,
    answer: String,
    score: i64,
    bold: Regex,
}

impl Question {
    fn new(question: String, answer: String, score: i64) -> Self {
        Question {
            question,
            answer,
            score,
            bold: Regex::new(r"(\[B\])|(\[\/B])").unwrap(),
        }
    }
}

impl Display for Question {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{} ({} літер) [+{}]",
            self.bold.replace_all(&self.question, "**"),
            self.answer.chars().count(),
            self.score
        ))
    }
}

fn next_question(r: Row) -> Question {
    let new_answer = r.read::<&str, _>("word").replace(|c: char| c == '\"', "");
    println!("{new_answer}");
    Question::new(
        r.read::<&str, _>("interpretation").to_string(),
        new_answer,
        r.read::<i64, _>("id_syn"),
    )
}

fn get_score(db: &Connection, user: u64) -> (i64, usize, usize) {
    let q = format!("SELECT * FROM scores WHERE user == {user}");
    let data = db.prepare(&q).unwrap().into_iter().last().unwrap().unwrap();
    let score = data.read::<i64, _>("score");
    let mut all_scores = db
        .prepare("SELECT * FROM scores")
        .unwrap()
        .into_iter()
        .map(|r| r.unwrap().read::<i64, _>("score"))
        .collect::<Vec<i64>>();
    all_scores.sort();
    let standing = all_scores.iter().position(|n| *n == score).unwrap() + 1;
    (score, standing, all_scores.len())
}

fn get_top(db: &Connection) -> Vec<(i64, i64)> {
    let mut accum = Vec::with_capacity(10);
    db.prepare("SELECT * FROM scores ORDER BY score DESC LIMIT 10")
        .unwrap()
        .into_iter()
        .map(|r| {
            let row = r.unwrap();
            let score = row.read::<i64, _>("score");
            let user = row.read::<i64, _>("user");
            accum.push((user, score));
        })
        .for_each(drop);
    accum
}

fn increment_score(db: &Connection, user: u64, score: i64) -> i64 {
    let current = format!("SELECT * FROM scores WHERE user == {user}");
    let ignore_if_exist = format!("INSERT OR IGNORE INTO scores (user, score) VALUES ({user}, 0)");
    db.execute(ignore_if_exist).unwrap();
    let data = db
        .prepare(&current)
        .unwrap()
        .into_iter()
        .last()
        .unwrap()
        .unwrap();
    let total_score = data.read::<i64, _>("score") + score;
    let insert = format!("INSERT OR REPLACE INTO scores VALUES ({user}, {total_score})");
    let _ = db.execute(&insert).unwrap();
    total_score
}

fn main() {
    println!("Opening DB");
    let mut db_path = env::var("CARGO_MANIFEST_DIR").unwrap_or(
        env::current_dir()
            .unwrap()
            .into_os_string()
            .into_string()
            .unwrap_or(".".into()),
    );
    db_path.push_str("/db/synsets_ua.db");
    println!("DB path: {}", &db_path);

    // Verb selector
    const QUERY: &str =
        "SELECT * FROM wlist WHERE interpretation IS NOT NULL AND word NOT LIKE '(%)'";
    const SCORE_TABLE_CREATE: &str =
        "CREATE TABLE IF NOT EXISTS scores (user INTEGER PRIMARY KEY UNIQUE, score INTEGER)";
    let db = sqlite::open(&db_path).expect("db expected");
    // Create if not present `score` table
    let _ = db.execute(SCORE_TABLE_CREATE).unwrap();
    let mut data: Vec<Row> = db
        .prepare(QUERY)
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
                let text = message.content.to_owned().trim().to_lowercase();
                println!(
                    "{}: {} says: {}",
                    message.timestamp, message.author.name, text
                );
                // service commands
                if text.chars().rev().last().is_some_and(|c| c.eq(&'!')) {
                    if text == "!next" || text == "!далі" || text == "!відповідь" {
                        let _ = discord.send_message(
                            message.channel_id,
                            &current_question.answer,
                            "",
                            false,
                        );
                        current_question = next_question(data.pop().unwrap());
                        let _ = discord.send_message(
                            message.channel_id,
                            &current_question.to_string(),
                            "",
                            false,
                        );
                    } else if text == "!q" || text == "!питання" || text == "!п" {
                        let _ = discord.send_message(
                            message.channel_id,
                            &current_question.to_string(),
                            "",
                            false,
                        );
                    } else if text == "!підказка" || text == "!хінт" {
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
                    } else if text == "!рейтинг" {
                        let (score, standing, total) = get_score(&db, message.author.id.0);
                        let _ = discord.send_message(
                            message.channel_id,
                            &format!(
                                "{} має {} очок і є {} зі {}",
                                message.author.mention(),
                                score,
                                standing,
                                total
                            ),
                            "",
                            false,
                        );
                    } else if text == "!топ" {
                        let top = get_top(&db);
                        let mut top_report = String::default();
                        top.into_iter()
                            .enumerate()
                            .map(|(id, (user, score))| {
                                top_report.push_str(
                                    format!(
                                        "{}    |    {}    |    {}\n",
                                        id + 1,
                                        discord
                                            .get_user(discord::model::UserId(
                                                user.try_into().unwrap()
                                            ))
                                            .unwrap()
                                            .mention(),
                                        score,
                                    )
                                    .as_str(),
                                );
                            })
                            .for_each(drop);
                        let _ = discord.send_message(message.channel_id, &top_report, "", false);
                    }
                } else if text.contains(&current_question.answer) {
                    // ansver verify and update score
                    let new_score =
                        increment_score(&db, message.author.id.0, current_question.score);
                    let _ = discord.send_message(
                        message.channel_id,
                        format!(
                            "Вірно {}. Відповідь {}. Загальний рейтинг: {}",
                            message.author.mention(),
                            current_question.answer,
                            new_score
                        )
                        .as_str(),
                        "",
                        false,
                    );
                    current_question = next_question(data.pop().unwrap());
                    let _ = discord.send_message(
                        message.channel_id,
                        &current_question.to_string(),
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

#[test]
fn bold_test() {
    let r = Regex::new(r"(\[B\])|(\[\/B])").unwrap();
    let res = r.replace_all("Те саме, що [B]заванта́жувати[/B]", "**");
    assert_eq!(res, "Те саме, що **заванта́жувати**")
}
