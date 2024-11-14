mod registry;
mod ui;

use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use std::{io, process};
use tui::backend::CrosstermBackend;
use tui::Terminal;
use clap::{Command, Arg, ArgAction};


#[tokio::main]
async fn main() -> Result<(), io::Error> {
    env_logger::init();
    let matches = Command::new("repo-tree")
        .version("1.0")
        .about("Docker Registry Tree Viewer")
        .arg(
            Arg::new("registry")
                .short('r')
                .long("registry")
                .value_name("URL")
                .help("Sets the Docker registry URL")
                .action(ArgAction::Set)
                .default_value("http://igloo.airgap.registry"),
        )
        .get_matches();

    // 인수로 받은 registry URL을 사용
    let registry_url = matches.get_one::<String>("registry").unwrap();
    registry::set_registry_url(registry_url);

    let images = registry::fetch_images().await.unwrap_or_else(|_| vec![]);

    if images.is_empty() {
        eprintln!("Warning: Could not connect to the registry at '{}'.", registry_url);
        eprintln!("Please check the registry URL or add the '--registry <URL>' option to specify a valid Docker registry.");
        process::exit(1);
    } else {
        // app_items이 비어 있지 않을 경우의 로직 처리
        println!("Registry items loaded successfully.");
    }

    let grouped_images = registry::group_images_by_depth(images);

    // 각 2뎁스 이미지에 대해 태그를 불러와 3뎁스 추가
    let mut app_items: Vec<(String, Vec<(String, Vec<String>)>)> = Vec::new();

    for (depth1, depth2_list) in grouped_images {
        let mut depth2_with_tags = Vec::new();
        for depth2 in depth2_list {
            let tags = registry::fetch_tags(&format!("{}/{}", depth1, depth2))
                .await
                .unwrap_or_else(|_| vec![]);
            depth2_with_tags.push((depth2, tags));
        }
        app_items.push((depth1, depth2_with_tags));
    }

    // 터미널 설정
    enable_raw_mode()?;
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // UI 실행
    let app = ui::App::new(app_items);
    let res = ui::run_app(&mut terminal, app).await;

    // 종료 후 터미널 복구
    disable_raw_mode()?;
    terminal.show_cursor()?;

    res
}