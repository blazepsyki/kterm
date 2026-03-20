use iced::{Color, Element, Length, Rectangle, Size, Theme};
use iced::advanced::{
    input_method::{InputMethod, Purpose},
    layout, mouse, renderer,
    widget::{tree, Widget, Tree},
    Clipboard, Layout, Shell,
};
use iced::widget::canvas::{Cache, Geometry, Program, Text};
use iced::{Point, Renderer, Vector};
use iced::mouse::Cursor;
use unicode_width::UnicodeWidthChar;
use vte::{Parser, Perform};

pub const D2CODING: iced::Font = iced::Font {
    family: iced::font::Family::Name("D2Coding"),
    ..iced::Font::DEFAULT
};

const ROW_HEIGHT: f32 = 20.0;
const CHAR_WIDTH: f32 = 10.0; // D2Coding은 0.5 비율이 적당합니다 (기존 12.0에서 축소)

// ─── 터미널 셀 ────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self { 
            ch: ' ', 
            fg: Color::WHITE, 
            bg: Color::BLACK,
            bold: false,
            italic: false,
            underline: false,
        }
    }
}

// ─── 터미널 라인 ─────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct Line {
    pub cells: Vec<Cell>,
    pub is_wrapped: bool, // 이전 라인에서 자동 줄바꿈되어 이어진 경우 true
}

impl Line {
    pub fn new(cols: usize) -> Self {
        Self {
            cells: vec![Cell::default(); cols],
            is_wrapped: false,
        }
    }
}

// ─── 터미널 상태 ──────────────────────────────────────────────────────────────
pub struct TerminalEmulator {
    pub cols: usize,
    pub rows: usize,
    pub history: std::collections::VecDeque<Line>, // 화면 위로 밀려난 내역
    pub grid: std::collections::VecDeque<Line>,    // 현재 화면에 보이는 라인들
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub current_fg: Color,
    pub current_bg: Color,
    pub current_bold: bool,
    pub current_italic: bool,
    pub current_underline: bool,
    pub scrolling_region: (usize, usize), // (top, bottom) - 0-indexed, inclusive
    pub cache: Cache,
    pub parser: Parser,
    pub ime_preedit: String,
    pub display_offset: usize, // 스크롤 위치 (0: 최하단)
    pub pending_responses: Vec<Vec<u8>>, // 터미널이 프로세스에게 보낼 응답 (예: 커서 위치)
}

impl Default for TerminalEmulator {
    fn default() -> Self { Self::new(24, 80) }
}

impl TerminalEmulator {
    pub fn new(rows: usize, cols: usize) -> Self {
        let mut grid = std::collections::VecDeque::with_capacity(rows);
        for _ in 0..rows {
            grid.push_back(Line::new(cols));
        }

        Self {
            cols, rows,
            history: std::collections::VecDeque::with_capacity(1000), // 가단위 1000줄
            grid,
            cursor_x: 0, cursor_y: 0,
            current_fg: Color::WHITE,
            current_bg: Color::BLACK,
            current_bold: false,
            current_italic: false,
            current_underline: false,
            scrolling_region: (0, rows - 1),
            cache: Cache::default(),
            parser: Parser::new(),
            ime_preedit: String::new(),
            display_offset: 0,
            pending_responses: Vec::new(),
        }
    }

    pub fn process_bytes(&mut self, bytes: &[u8]) {
        let mut parser = std::mem::replace(&mut self.parser, Parser::new());
        let mut performer = TerminalPerformer { emulator: self };
        parser.advance(&mut performer, bytes);
        performer.emulator.parser = parser;
        performer.emulator.cache.clear();
    }

    pub fn clear_preedit(&mut self) {
        self.ime_preedit.clear();
        self.cache.clear();
    }

    fn scroll_up_and_push_history(&mut self) {
        if let Some(line) = self.grid.pop_front() {
            self.history.push_back(line);
            // 무제한 스크롤백을 위해 히스토리 크기 제한을 두지 않거나 매우 크게 설정
            if self.history.len() > 10000 {
                self.history.pop_front();
            }
        }
        self.grid.push_back(Line::new(self.cols));
    }

    fn scroll_up_in_region(&mut self, top: usize, bottom: usize) {
        if top == 0 && bottom == self.rows - 1 {
            self.scroll_up_and_push_history();
        } else {
            // Region scroll-up
            if top < bottom && bottom < self.rows {
                let _ = self.grid.remove(top);
                self.grid.insert(bottom, Line::new(self.cols));
            }
        }
    }

    fn scroll_down_in_region(&mut self, top: usize, bottom: usize) {
        if top < bottom && bottom < self.rows {
            let _ = self.grid.remove(bottom);
            self.grid.insert(top, Line::new(self.cols));
        }
    }

    fn clear_line(&mut self, row: usize, mode: usize) {
        let blank = Cell { 
            ch: ' ', 
            fg: self.current_fg, 
            bg: self.current_bg,
            bold: false,
            italic: false,
            underline: false,
        };
        if row < self.grid.len() {
            let line = &mut self.grid[row];
            match mode {
                0 => { // To end
                    for x in self.cursor_x..self.cols { line.cells[x] = blank; }
                }
                1 => { // To start
                    for x in 0..=self.cursor_x { if x < self.cols { line.cells[x] = blank; } }
                }
                2 => { // All
                    for x in 0..self.cols { line.cells[x] = blank; }
                }
                _ => {}
            }
        }
    }

    fn clear_screen(&mut self, mode: usize) {
        match mode {
            0 => { // Below
                self.clear_line(self.cursor_y, 0);
                for y in (self.cursor_y + 1)..self.rows {
                    self.clear_line(y, 2);
                }
            }
            1 => { // Above
                for y in 0..self.cursor_y {
                    self.clear_line(y, 2);
                }
                self.clear_line(self.cursor_y, 1);
            }
            2 => { // All
                for y in 0..self.rows { self.clear_line(y, 2); }
                self.cursor_x = 0;
                self.cursor_y = 0;
            }
            _ => {}
        }
    }

    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        if self.cols == new_cols && self.rows == new_rows { return; }

        let mut all_lines: Vec<Line> = self.history.drain(..).collect();
        all_lines.extend(self.grid.drain(..));

        let mut logical_lines: Vec<Vec<Cell>> = Vec::new();
        let mut current_logical: Vec<Cell> = Vec::new();

        for line in all_lines {
            if !line.is_wrapped && !current_logical.is_empty() {
                logical_lines.push(current_logical);
                current_logical = Vec::new();
            }
            // 공백 트리밍은 생략하거나 주의 필요 (커서 위치 보존 때문)
            current_logical.extend(line.cells);
        }
        if !current_logical.is_empty() {
            logical_lines.push(current_logical);
        }

        let mut new_all_lines = Vec::new();
        for mut logical in logical_lines {
            // 오른쪽 끝 공백 제거 (Reflow 최적화)
            while logical.last().map_or(false, |c| c.ch == ' ') {
                logical.pop();
            }
            
            if logical.is_empty() {
                new_all_lines.push(Line::new(new_cols));
                continue;
            }

            let chunks = logical.chunks(new_cols);
            for (i, chunk) in chunks.enumerate() {
                let mut new_line = Line::new(new_cols);
                for (j, &cell) in chunk.iter().enumerate() {
                    new_line.cells[j] = cell;
                }
                new_line.is_wrapped = i > 0;
                new_all_lines.push(new_line);
            }
        }

        self.cols = new_cols;
        self.rows = new_rows;

        if new_all_lines.len() <= self.rows {
            self.grid = new_all_lines.into();
            while self.grid.len() < self.rows {
                self.grid.push_back(Line::new(self.cols));
            }
        } else {
            let split_at = new_all_lines.len() - self.rows;
            self.history = new_all_lines.drain(..split_at).collect();
            self.grid = new_all_lines.into();
        }

        self.scrolling_region = (0, self.rows - 1);
        self.cursor_x = std::cmp::min(self.cursor_x, self.cols - 1);
        self.cursor_y = std::cmp::min(self.cursor_y, self.rows - 1);
        // display_offset이 히스토리 범위를 초과하지 않도록 클램핑
        self.display_offset = std::cmp::min(self.display_offset, self.history.len());
        self.cache.clear();
    }

    /// 커서의 픽셀 범위 (OS IME 팝업 힌트용)
    pub fn cursor_rect_in(&self, layout_bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: layout_bounds.x + self.cursor_x as f32 * CHAR_WIDTH,
            y: layout_bounds.y + self.cursor_y as f32 * ROW_HEIGHT,
            width: CHAR_WIDTH,
            height: ROW_HEIGHT,
        }
    }

    /// 현재 InputMethod 상태값 반환 (preedit: None → Iced 런타임 자체 오버레이 비활성화)
    /// preedit 렌더링은 Canvas draw()에서 직접 처리하므로, Iced에는 커서 위치만 전달한다.
    pub fn current_input_method(&self, bounds: Rectangle) -> InputMethod {
        InputMethod::Enabled {
            cursor: self.cursor_rect_in(bounds),
            purpose: Purpose::Terminal,
            preedit: None, // On-the-spot: 우리가 캔버스에서 직접 그림
        }
    }
}

// ANSI 색상 매핑
fn ansi_color(index: u8) -> Color {
    match index {
        0 => Color::BLACK,
        1 => Color::from_rgb8(205, 0, 0),    // Red
        2 => Color::from_rgb8(0, 205, 0),    // Green
        3 => Color::from_rgb8(205, 205, 0),  // Yellow
        4 => Color::from_rgb8(50, 50, 255),  // Blue
        5 => Color::from_rgb8(205, 0, 205),  // Magenta
        6 => Color::from_rgb8(0, 205, 205),  // Cyan
        7 => Color::WHITE,
        _ => Color::WHITE,
    }
}

fn ansi_color_bright(index: u8) -> Color {
    match index {
        0 => Color::from_rgb8(127, 127, 127),
        1 => Color::from_rgb8(255, 0, 0),
        2 => Color::from_rgb8(0, 255, 0),
        3 => Color::from_rgb8(255, 255, 0),
        4 => Color::from_rgb8(92, 92, 255),
        5 => Color::from_rgb8(255, 0, 255),
        6 => Color::from_rgb8(0, 255, 255),
        7 => Color::WHITE,
        _ => Color::WHITE,
    }
}

fn parse_extended_color(params: &mut vte::ParamsIter<'_>) -> Option<Color> {
    let type_param = params.next()?.first()?;
    match type_param {
        5 => { // 256 colors
            let index = (*params.next()?.first()?) as u8;
            Some(color_from_256(index))
        }
        2 => { // RGB
            let r = (*params.next()?.first()?) as u8;
            let g = (*params.next()?.first()?) as u8;
            let b = (*params.next()?.first()?) as u8;
            Some(Color::from_rgb8(r, g, b))
        }
        _ => None,
    }
}

fn color_from_256(index: u8) -> Color {
    match index {
        0..=7 => ansi_color(index),
        8..=15 => ansi_color_bright(index - 8),
        16..=231 => {
            let index = index - 16;
            let r = (index / 36) * 51;
            let g = ((index / 6) % 6) * 51;
            let b = (index % 6) * 51;
            Color::from_rgb8(r, g, b)
        }
        232..=255 => {
            let gray = (index - 232) * 10 + 8;
            Color::from_rgb8(gray, gray, gray)
        }
    }
}

// ─── vte::Perform ─────────────────────────────────────────────────────────────
struct TerminalPerformer<'a> { emulator: &'a mut TerminalEmulator }

impl<'a> Perform for TerminalPerformer<'a> {
    fn print(&mut self, c: char) {
        let emu = &mut self.emulator;

        // 1. 래핑 처리: 현재 X 위치가 가득 찼으면 다음 줄로
        if emu.cursor_x >= emu.cols {
            emu.cursor_x = 0;
            let (_, bottom) = emu.scrolling_region;
            if emu.cursor_y < bottom {
                emu.cursor_y += 1;
                emu.grid[emu.cursor_y].is_wrapped = true;
            } else if emu.cursor_y == bottom {
                let (top, bottom) = emu.scrolling_region;
                emu.scroll_up_in_region(top, bottom);
                emu.grid[bottom].is_wrapped = true;
            } else {
                // Outside region, just move down if possible
                if emu.cursor_y < emu.rows - 1 {
                    emu.cursor_y += 1;
                }
            }
        }

        let w = c.width().unwrap_or(1);
        if emu.cursor_y < emu.rows && emu.cursor_x < emu.cols {
            let line = &mut emu.grid[emu.cursor_y];
            line.cells[emu.cursor_x] = Cell { 
                ch: c, 
                fg: emu.current_fg, 
                bg: emu.current_bg,
                bold: emu.current_bold,
                italic: emu.current_italic,
                underline: emu.current_underline,
            };
            if w > 1 && emu.cursor_x + 1 < emu.cols {
                line.cells[emu.cursor_x + 1] = Cell { 
                    ch: '\0', 
                    fg: emu.current_fg, 
                    bg: emu.current_bg,
                    bold: emu.current_bold,
                    italic: emu.current_italic,
                    underline: emu.current_underline,
                };
            }
            emu.cursor_x += w;
        }
    }

    fn execute(&mut self, byte: u8) {
        let emu = &mut self.emulator;
        match byte {
            b'\n' => { 
                let (top, bottom) = emu.scrolling_region;
                if emu.cursor_y == bottom {
                    emu.scroll_up_in_region(top, bottom);
                } else if emu.cursor_y < emu.rows - 1 {
                    emu.cursor_y += 1;
                }
                emu.grid[emu.cursor_y].is_wrapped = false; // 하드 엔터
            }
            b'\r'   => emu.cursor_x = 0,
            b'\x08' => { // Backspace
                if emu.cursor_x > 0 {
                    emu.cursor_x -= 1;
                    if emu.grid[emu.cursor_y].cells[emu.cursor_x].ch == '\0' && emu.cursor_x > 0 {
                        emu.cursor_x -= 1;
                    }
                }
            }
            b'\x07' => { /* Bell */ }
            b'\t' => { // Tab
                let spaces = 8 - (emu.cursor_x % 8);
                for _ in 0..spaces { self.print(' '); }
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore || !intermediates.is_empty() { return; }
        let emu = &mut self.emulator;
        let mut it = params.iter();
        let param1 = it.next().and_then(|p| p.first()).copied().unwrap_or(0) as usize;

        match action {
            'A' => emu.cursor_y = emu.cursor_y.saturating_sub(std::cmp::max(1, param1)),
            'B' => emu.cursor_y = std::cmp::min(emu.rows - 1, emu.cursor_y + std::cmp::max(1, param1)),
            'C' => emu.cursor_x = std::cmp::min(emu.cols - 1, emu.cursor_x + std::cmp::max(1, param1)),
            'D' => emu.cursor_x = emu.cursor_x.saturating_sub(std::cmp::max(1, param1)),
            'H' | 'f' => { // CUP, HVP
                let y = param1;
                let x = it.next().and_then(|p| p.first()).copied().unwrap_or(1) as usize;
                emu.cursor_y = std::cmp::min(emu.rows - 1, y.saturating_sub(1));
                emu.cursor_x = std::cmp::min(emu.cols - 1, x.saturating_sub(1));
            }
            'm' => { // SGR - Colors and Attributes
                let mut params_it = params.iter();
                while let Some(param) = params_it.next() {
                    let p = param[0];
                    match p {
                        0 => { 
                            emu.current_fg = Color::WHITE; 
                            emu.current_bg = Color::BLACK; 
                            emu.current_bold = false;
                            emu.current_italic = false;
                            emu.current_underline = false;
                        }
                        1 => emu.current_bold = true,
                        3 => emu.current_italic = true,
                        4 => emu.current_underline = true,
                        22 => emu.current_bold = false,
                        23 => emu.current_italic = false,
                        24 => emu.current_underline = false,
                        30..=37 => emu.current_fg = ansi_color((p - 30) as u8),
                        38 => { // Extended FG
                            if let Some(color) = parse_extended_color(&mut params_it) {
                                emu.current_fg = color;
                            }
                        }
                        39 => emu.current_fg = Color::WHITE,
                        40..=47 => emu.current_bg = ansi_color((p - 40) as u8),
                        48 => { // Extended BG
                            if let Some(color) = parse_extended_color(&mut params_it) {
                                emu.current_bg = color;
                            }
                        }
                        49 => emu.current_bg = Color::BLACK,
                        90..=97 => emu.current_fg = ansi_color_bright((p - 90) as u8),
                        100..=107 => emu.current_bg = ansi_color_bright((p - 100) as u8),
                        _ => {}
                    }
                }
            }
            'J' => emu.clear_screen(param1), // ED
            'K' => emu.clear_line(emu.cursor_y, param1), // EL
            'P' => { // DCH (Delete Character)
                let n = std::cmp::max(1, param1);
                let y = emu.cursor_y;
                let x = emu.cursor_x;
                if y < emu.grid.len() {
                    let line = &mut emu.grid[y];
                    for i in x..emu.cols {
                        if i + n < emu.cols {
                            line.cells[i] = line.cells[i + n];
                        } else {
                            line.cells[i] = Cell { 
                                ch: ' ', 
                                fg: emu.current_fg, 
                                bg: emu.current_bg,
                                bold: false,
                                italic: false,
                                underline: false,
                            };
                        }
                    }
                }
            }
            'L' => { // IL (Insert Line)
                let n = std::cmp::max(1, param1);
                for _ in 0..n {
                    emu.scroll_down_in_region(emu.cursor_y, emu.scrolling_region.1);
                }
            }
            'M' => { // DL (Delete Line)
                let n = std::cmp::max(1, param1);
                for _ in 0..n {
                    emu.scroll_up_in_region(emu.cursor_y, emu.scrolling_region.1);
                }
            }
            'S' => { // SU (Scroll Up)
                let n = std::cmp::max(1, param1);
                for _ in 0..n {
                    emu.scroll_up_in_region(emu.scrolling_region.0, emu.scrolling_region.1);
                }
            }
            'T' => { // SD (Scroll Down)
                let n = std::cmp::max(1, param1);
                for _ in 0..n {
                    emu.scroll_down_in_region(emu.scrolling_region.0, emu.scrolling_region.1);
                }
            }
            'r' => { // DECSTBM
                let top = param1.saturating_sub(1);
                let bottom = it.next().and_then(|p| p.first()).copied().unwrap_or(emu.rows as u16) as usize;
                let bottom = std::cmp::min(bottom.saturating_sub(1), emu.rows - 1);
                if top < bottom {
                    emu.scrolling_region = (top, bottom);
                }
            }
            'n' => { // DSR - Device Status Report
                if param1 == 6 {
                    let response = format!("\x1b[{};{}R", emu.cursor_y + 1, emu.cursor_x + 1);
                    emu.pending_responses.push(response.into_bytes());
                }
            }
            'c' => { // DA - Device Attributes
                // "I am a VT100 with advanced features"
                emu.pending_responses.push(b"\x1b[?1;2c".to_vec());
            }
            _ => {}
        }
    }
}

// ─── Canvas Program (렌더링) ───────────────────────────────────────────────────
impl<Message> Program<Message> for TerminalEmulator {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: Cursor) -> Vec<Geometry> {
        let geo = self.cache.draw(renderer, bounds.size(), |frame| {
            // println!("[Terminal] Drawing frame, bounds: {:?}", bounds);
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::BLACK);
            
            // 그릴 라인 선택 (히스토리 + 현시점 그리드)
            let mut all_viewable: Vec<&Line> = self.history.iter().collect();
            all_viewable.extend(self.grid.iter());

            let start_idx = all_viewable.len().saturating_sub(self.rows + self.display_offset);
            let end_idx = std::cmp::min(start_idx + self.rows, all_viewable.len());
            let visible_lines = &all_viewable[start_idx..end_idx];

            for (y, row) in visible_lines.iter().enumerate() {
                for (x, cell) in row.cells.iter().enumerate() {
                    let pos = Point::new(x as f32 * CHAR_WIDTH, y as f32 * ROW_HEIGHT);
                    
                    // 1. 배경색 렌더링 (검정색이 아닐 때만)
                    if cell.bg != Color::BLACK {
                        frame.fill_rectangle(pos, Size::new(CHAR_WIDTH, ROW_HEIGHT), cell.bg);
                    }

                    // 2. 텍스트 렌더링
                    if cell.ch != ' ' && cell.ch != '\0' {
                        let mut t = Text::default();
                        t.content = cell.ch.to_string();
                        t.position = pos;
                        t.color = cell.fg;
                        t.size = iced::Pixels(ROW_HEIGHT * 0.95);
                        
                        // Bold 속성 반영
                        t.font = if cell.bold {
                            iced::Font { weight: iced::font::Weight::Bold, ..D2CODING }
                        } else {
                            D2CODING
                        };
                        
                        frame.fill_text(t);
                    }

                    // 3. 밑줄(Underline) 렌더링
                    if cell.underline {
                        frame.fill_rectangle(
                            Point::new(pos.x, pos.y + ROW_HEIGHT - 2.0),
                            Size::new(CHAR_WIDTH, 1.0),
                            cell.fg
                        );
                    }
                }
            }

            // 커서는 항상 현재 입력 중인 (최하단) 그리드에 위치하므로 오프셋에 따라 숨김 처리
            if self.display_offset == 0 {
                let cp = Point::new(self.cursor_x as f32 * CHAR_WIDTH, self.cursor_y as f32 * ROW_HEIGHT);
                frame.fill_rectangle(cp, Size::new(CHAR_WIDTH, ROW_HEIGHT), Color::from_rgba(0.5, 1.0, 0.5, 0.5));
                if !self.ime_preedit.is_empty() {
                    let pw = self.ime_preedit.chars().count() as f32 * CHAR_WIDTH;
                    frame.fill_rectangle(cp, Size::new(pw, ROW_HEIGHT), Color::BLACK);
                    let mut t = Text::default();
                    t.content = self.ime_preedit.clone();
                    t.position = cp;
                    t.color = Color::WHITE;
                    t.size = iced::Pixels(ROW_HEIGHT * 0.95);
                    t.font = D2CODING;
                    frame.fill_text(t);
                }
            }
        });
        vec![geo]
    }
}

// ─── 공식 커스텀 Widget ───────────────────────────────────────────────────────
/// Canvas 기반 렌더링 + Widget::update()에서 shell.request_input_method()로
/// OS에 터미널 커서 정확한 픽셀 좌표 전달 → 한글 팝업 위치 정확
pub struct TerminalView<'a, Message> {
    emulator: &'a TerminalEmulator,
    on_scroll: fn(f32) -> Message,
    on_resize: fn(usize, usize) -> Message,
}

impl<'a, Message> TerminalView<'a, Message> {
    pub fn new(emulator: &'a TerminalEmulator, on_scroll: fn(f32) -> Message, on_resize: fn(usize, usize) -> Message) -> Self {
        Self { emulator, on_scroll, on_resize }
    }
}

impl<'a, Message: Clone> Widget<Message, Theme, Renderer> for TerminalView<'a, Message> {
    fn tag(&self) -> tree::Tag { tree::Tag::stateless() }
    fn state(&self) -> tree::State { tree::State::None }
    fn children(&self) -> Vec<Tree> { vec![] }
    fn diff(&self, _tree: &mut Tree) {}

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if bounds.width < 1.0 || bounds.height < 1.0 { return; }
        let layers = <TerminalEmulator as Program<Message>>::draw(self.emulator, &(), renderer, theme, bounds, cursor);
        use iced::advanced::Renderer as AR;
        AR::with_translation(renderer, Vector::new(bounds.x, bounds.y), |renderer: &mut Renderer| {
            use iced::advanced::graphics::geometry::Renderer as GR2;
            for layer in layers {
                GR2::draw_geometry(renderer, layer);
            }
        });
    }

    /// ★ 공식 Iced IME 통합 핵심:
    /// RedrawRequested 이벤트 때마다 shell.request_input_method()를 호출하여
    /// OS/Iced 런타임에 현재 터미널 커서 픽셀 위치를 알린다.
    fn update(
        &mut self,
        _tree: &mut Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let new_cols = (bounds.width / 10.0) as usize;
        let new_rows = (bounds.height / 20.0) as usize;

        if new_cols > 0 && new_rows > 0 && (new_cols != self.emulator.cols || new_rows != self.emulator.rows) {
            // main.rs로 리사이즈 요청 전송
            // on_scroll처럼 콜백을 하나 더 추가하거나, TerminalResize 전용 콜백 도입 필요
            // 여기서는 단순함을 위해 기존 TerminalResize 메시지를 활용하도록 main.rs 수정 필요
        }

        match event {
            iced::Event::Window(iced::window::Event::RedrawRequested(_)) => {
                let ime_state = self.emulator.current_input_method(bounds);
                shell.request_input_method(&ime_state);
                
                if new_cols > 0 && new_rows > 0 && (new_cols != self.emulator.cols || new_rows != self.emulator.rows) {
                    shell.publish((self.on_resize)(new_rows, new_cols));
                }
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                match delta {
                    mouse::ScrollDelta::Lines { y, .. } | mouse::ScrollDelta::Pixels { y, .. } => {
                        shell.publish((self.on_scroll)(*y));
                    }
                }
            }
            _ => {}
        }
    }

    fn operate(&mut self, _tree: &mut Tree, _layout: Layout<'_>, _renderer: &Renderer, _op: &mut dyn iced::advanced::widget::Operation<()>) {}

    fn mouse_interaction(&self, _tree: &Tree, _layout: Layout<'_>, _cursor: mouse::Cursor, _viewport: &Rectangle, _renderer: &Renderer) -> mouse::Interaction {
        mouse::Interaction::default()
    }
}

impl<'a, Message: Clone + 'a> From<TerminalView<'a, Message>> for Element<'a, Message> {
    fn from(w: TerminalView<'a, Message>) -> Self { Element::new(w) }
}
