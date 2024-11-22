use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};

use tui::widgets::Clear as PopupClear;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::{execute, terminal::{Clear, ClearType}};

use std::io;
use crate::registry;

use tui::backend::CrosstermBackend;
use std::io::Stdout;
use serde_json::Value;
use tabled::{ Tabled, Table, settings::{Style as TStyle, Modify, object::Columns, Alignment as TAlignment}};
    
#[derive(Tabled)]
pub struct CompatibilityRow {
    #[tabled(rename = "id")]
    pub id: String,

    #[tabled(rename = "parent")]
    pub parent: String,

    pub os: String,

    pub created: String,

    #[tabled(rename = "Cmd")]
    pub cmd: String,

    pub config: String,
}

#[derive(Tabled)]
pub struct LayerInfo {
    #[tabled(rename = "BlobSum (Digest)")]
    pub blob_sum: String,
    
    #[tabled(rename = "Size")]
    pub size: String,
    
    #[tabled(rename = "Command")]
    pub command: String,
}

pub struct App {
    pub items: Vec<String>,
    pub item_types: Vec<usize>,
    pub full_image_names: Vec<String>,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub popup_open: bool,
    pub popup_content: String,
    pub popup_scroll_offset: usize, // 팝업 내부 스크롤 위치
    pub popup_scroll_offset_x: usize, // 수평 스크롤 오프셋 추가
}

impl App {
    pub fn new(raw_items: Vec<(String, Vec<(String, Vec<String>)>)>) -> App {
        let mut items = Vec::new();
        let mut item_types = Vec::new();
        let mut full_image_names = Vec::new(); // 풀 이미지 이름 목록

        for (i, (depth1, depth2_list)) in raw_items.iter().enumerate() {
            let host = depth1.clone();
            let is_last_host = i == raw_items.len() - 1;
        
            // 호스트 항목 추가 (1뎁스)
            let host_prefix = if is_last_host { "└── " } else { "├── " };
            items.push(format!("{}{}", host_prefix, depth1));
            item_types.push(1);
            full_image_names.push(host.clone());
        
            for (j, (depth2, tags)) in depth2_list.iter().enumerate() {
                let repo_name = format!("{}/{}", host, depth2);
                let is_last_repo = j == depth2_list.len() - 1;
        
                // 이미지명 항목 추가 (2뎁스)
                let depth2_prefix = if is_last_host { "    " } else { "│   " };
                let repo_prefix = if is_last_repo { "└── " } else { "├── " };
                items.push(format!("{}{}{}", depth2_prefix, repo_prefix, depth2));
                item_types.push(2);
                full_image_names.push(repo_name.clone());
        
                for (k, tag) in tags.iter().enumerate() {
                    let is_last_tag = k == tags.len() - 1;
        
                    // 태그 항목 추가 (3뎁스)
                    let tag_prefix = if is_last_repo {
                        if is_last_tag { format!("{}    └── ", depth2_prefix) } else { format!("{}    ├── ", depth2_prefix) }
                    } else {
                        if is_last_tag { format!("{}│   └── ", depth2_prefix) } else { format!("{}│   ├── ", depth2_prefix) }
                    };
                    let full_image_name = format!("{}/{}", repo_name, tag);
                    items.push(format!("{}{}", tag_prefix, tag));
                    item_types.push(3);
                    full_image_names.push(full_image_name);
                }
            }
        }

        App {
            items,
            item_types,
            full_image_names, // 풀 이미지 이름 필드에 추가
            selected_index: 0,
            scroll_offset: 0,
            popup_open: false,
            popup_content: String::new(),
            popup_scroll_offset: 0,
            popup_scroll_offset_x: 0,
        }
    }

    pub fn next(&mut self, max_visible_items: usize) {
        if self.selected_index + 1 < self.items.len() {
            self.selected_index += 1;
            if self.selected_index >= self.scroll_offset + max_visible_items {
                self.scroll_offset += 1;
            }
        }
    }

    pub fn previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            if self.selected_index < self.scroll_offset {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
        }
    }

    pub fn next_page(&mut self, max_visible_items: usize) {
        // 한 페이지 만큼 아래로 이동
        if self.selected_index + max_visible_items < self.items.len() {
            self.selected_index += max_visible_items;
        } else {
            self.selected_index = self.items.len() - 1; // 마지막 항목에 도달
        }

        // 새 selected_index가 화면에 보이도록 scroll_offset 조정
        self.scroll_offset = self.selected_index.saturating_sub(max_visible_items - 1);
    }

    pub fn previous_page(&mut self, max_visible_items: usize) {
        // 한 페이지 만큼 위로 이동
        if self.selected_index >= max_visible_items {
            self.selected_index -= max_visible_items;
        } else {
            self.selected_index = 0; // 첫 번째 항목으로 이동
        }

        // 새 selected_index가 화면에 보이도록 scroll_offset 조정
        self.scroll_offset = self.selected_index;
    }
    
    pub async fn open_popup(&mut self) {
        if self.item_types[self.selected_index] == 3 {
            let full_image_name = &self.full_image_names[self.selected_index];
            let parts: Vec<&str> = full_image_name.rsplitn(2, '/').collect();
            let tag_name = parts[0];
            let image_name = parts[1];
        
            if let Ok(manifest_json) = registry::fetch_manifest(image_name, tag_name).await {
                let manifest_value: Value = serde_json::from_str(&manifest_json).unwrap();
    
                // CompatibilityRow 테이블 데이터와 JSON 전체 문자열을 가져옴
                let (table_data, full_json) = registry::parse_v1compatibility_fields(&manifest_value);
    
                // `table_data`로 `tabled` 테이블 생성
                let mut table = Table::new(&table_data);
                table
                    .with(TStyle::modern())
                    .with(Modify::new(Columns::single(0)).with(TAlignment::left()))
                    .with(Modify::new(Columns::single(1)).with(TAlignment::left()));
    
                // popup_content에 테이블과 구분선, 전체 JSON 추가
                self.popup_content = format!("{}\n------------------------\n{}", table, full_json);
    
                self.popup_open = true;
                self.popup_scroll_offset = 0;
            }
        }
    }

    pub fn close_popup(&mut self) {
        self.popup_open = false;
        self.popup_content.clear();
    }
    
    pub fn handle_popup_input(&mut self, key: KeyEvent, max_visible_popup_lines: usize) {
        // 팝업 내용의 총 줄 수와 스크롤 가능한 최대 줄 수 계산
        let max_popup_lines = self.popup_content.lines().count();
        let max_scroll_offset = max_popup_lines.saturating_sub(max_visible_popup_lines);

        match key.code {
            KeyCode::Down => {
                // 한 줄 아래로 스크롤 (최대값 초과하지 않음)
                self.popup_scroll_offset = (self.popup_scroll_offset + 3).min(max_scroll_offset);
            }
            KeyCode::Up => {
                // 한 줄 위로 스크롤 (0 이하로 내려가지 않음)
                self.popup_scroll_offset = self.popup_scroll_offset.saturating_sub(3);
            }
            KeyCode::PageDown => {
                // 한 페이지 아래로 스크롤 (최대값 초과하지 않음)
                self.popup_scroll_offset = (self.popup_scroll_offset + max_visible_popup_lines).min(max_scroll_offset);
            }
            KeyCode::PageUp => {
                // 한 페이지 위로 스크롤 (0 이하로 내려가지 않음)
                self.popup_scroll_offset = self.popup_scroll_offset.saturating_sub(max_visible_popup_lines);
            },
            KeyCode::Right => {
                self.popup_scroll_offset_x += 10; // 오른쪽 스크롤
            }
            KeyCode::Left => {
                self.popup_scroll_offset_x = self.popup_scroll_offset_x.saturating_sub(10); // 왼쪽 스크롤
            }
            KeyCode::Esc => {
                // 팝업 닫기
                self.close_popup();
            }
            _ => {}
        }
    }

    pub async fn handle_main_input(&mut self, key: KeyEvent, max_visible_items: usize) {
        match key.code {
            KeyCode::Char('q') => {}
            KeyCode::Down => self.next(max_visible_items),
            KeyCode::Up => self.previous(),
            KeyCode::PageDown => self.next_page(max_visible_items),
            KeyCode::PageUp => self.previous_page(max_visible_items),
            KeyCode::Enter => {
                if self.popup_open {
                    self.close_popup();
                } else {
                    self.open_popup().await;
                }
            }
            KeyCode::Esc => {
                self.close_popup();
            }
            _ => {}
        }
    }

}

fn centered_rect(percent_x: u16, percent_y: u16, r: tui::layout::Rect) -> tui::layout::Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
pub fn render_ui<B: Backend>(f: &mut Frame<B>, app: &App) {
    let banner_text = vec![
        Spans::from("██████╗░███████╗██████╗░░█████╗░░░░░░░████████╗██████╗░███████╗███████╗"),
        Spans::from("██╔══██╗██╔════╝██╔══██╗██╔══██╗░░░░░░╚══██╔══╝██╔══██╗██╔════╝██╔════╝"),
        Spans::from("██████╔╝█████╗░░██████╔╝██║░░██║█████╗░░░██║░░░██████╔╝█████╗░░█████╗░░"),
        Spans::from("██╔══██╗██╔══╝░░██╔═══╝░██║░░██║╚════╝░░░██║░░░██╔══██╗██╔══╝░░██╔══╝░░"),
        Spans::from("██║░░██║███████╗██║░░░░░╚█████╔╝░░░░░░░░░██║░░░██║░░██║███████╗███████╗"),
        Spans::from("╚═╝░░╚═╝╚══════╝╚═╝░░░░░░╚════╝░░░░░░░░░░╚═╝░░░╚═╝░░╚═╝╚══════╝╚══════╝"),
    ];

    let usage_text = vec![
        Spans::from("Usage:"),
        Spans::from("  - Use arrow keys ↑/↓ to navigate"),
        Spans::from("  - Press Enter to open details"),
        Spans::from("  - Press Esc to close details"),
        Spans::from("  - Press q or Ctrl+C to quit"),
    ];

    let team_text = vec![
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::styled("Develop by Data Platform team  ", Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC))]),
    ];

    // 화면을 세로로 나누기: 상단에 배너, 하단에 트리 구조
    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),  // 상단 배너와 사용법의 높이
            Constraint::Min(0),     // 하단 트리의 최소 높이
        ].as_ref())
        .split(f.size());

    // 배너 섹션을 가로로 나누기: 왼쪽, 중앙, 오른쪽
    let banner_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(40), // 중앙에 사용 안내
            Constraint::Percentage(30), // 오른쪽에 팀 정보
        ].as_ref())
        .split(vertical_chunks[0]);

    // 배너 텍스트 렌더링
    f.render_widget(
        Paragraph::new(banner_text).block(Block::default().borders(Borders::TOP | Borders::BOTTOM | Borders::LEFT)),
        banner_chunks[0],
    );
    f.render_widget(
        Paragraph::new(usage_text).block(Block::default().borders(Borders::TOP | Borders::BOTTOM)),
        banner_chunks[1],
    );
    f.render_widget(
        Paragraph::new(team_text).block(Block::default().borders(Borders::TOP | Borders::BOTTOM | Borders::RIGHT))
        .alignment(Alignment::Right),
        banner_chunks[2],
    );

    // 트리 UI 구성
    let max_visible_items = (f.size().height as usize).saturating_sub(3);
    let items: Vec<ListItem> = app
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = match app.item_types[i] {
                1 => Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD), // 1뎁스
                2 => Style::default().fg(Color::Green),                               // 2뎁스
                3 => Style::default().fg(Color::Gray),                                // 3뎁스 (태그)
                _ => Style::default(),
            };

            let styled_item = if i == app.selected_index {
                Span::styled(item.clone(), style.bg(Color::Yellow).add_modifier(Modifier::BOLD | Modifier::ITALIC))
            } else {
                Span::styled(item.clone(), style)
            };
            ListItem::new(vec![Spans::from(styled_item)])
        })
        .collect();

    let visible_items = &items[app.scroll_offset..(app.scroll_offset + max_visible_items).min(items.len())];

    let list = List::new(visible_items.to_vec())
        .block(Block::default().borders(Borders::ALL).title("Docker Images Tree"))
        .highlight_style(Style::default().bg(Color::Yellow).add_modifier(Modifier::BOLD));

    // 하단 레이아웃에 트리 렌더링
    f.render_widget(list, vertical_chunks[1]);

    // 팝업이 열려 있으면 팝업 표시
    if app.popup_open {
        let popup = Paragraph::new(app.popup_content.clone())
            .block(
                Block::default()
                    .title("Tag Details")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Magenta)),
            )
            .style(Style::default().fg(Color::White).bg(Color::Black).add_modifier(Modifier::ITALIC))
            .scroll((app.popup_scroll_offset as u16, app.popup_scroll_offset_x as u16)); // 수평 스크롤 적용
        let area = centered_rect(80, 60, f.size());
        f.render_widget(PopupClear, area);
        f.render_widget(popup, area);
    }
}

pub async fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, mut app: App) -> io::Result<()> {
    // 터미널 화면 전체 초기화
    execute!(terminal.backend_mut(), Clear(ClearType::All))?;

    loop {
    
        terminal.draw(|f| render_ui(f, &app))?;

        // 터미널 크기에 따라 실제 팝업에 표시 가능한 최대 줄 수를 계산
        let popup_height = (terminal.size()?.height * 60 / 100) as usize; // 60% 높이에 맞춤
        let max_visible_popup_lines = popup_height.saturating_sub(2); // 여백 고려

        if let Event::Key(key) = event::read()? {
            if (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
            || key.code == KeyCode::Char('q') {
                execute!(terminal.backend_mut(), Clear(ClearType::All))?;
                return Ok(());
            }
            if app.popup_open {
                app.handle_popup_input(key, max_visible_popup_lines);
            } else {
                app.handle_main_input(key, max_visible_popup_lines).await;
            }
        }
    }
}