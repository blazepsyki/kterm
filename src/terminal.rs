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

// ─── 터미널 선택 영역 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub start: (usize, usize), // (col, row)
    pub end: (usize, usize),   // (col, row)
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
    pub selection: Option<Selection>,
    pub last_csi: Option<(char, usize, bool)>, // ConPTY 버그 우회를 위한 마지막 CSI 액션 기억
    pub history_prompt_y: Option<usize>, // 위로 점프한 프롬프트 Y좌표 기록 (잔상 청소기 용도)
    pub history_clear_timeout: usize, // 위 좌표의 유효 수명 (CSI 명령 횟수)
    pub history_printed_lines: std::collections::HashSet<usize>, // 잔상 청소 기간 중 텍스트가 인쇄된 라인 기록
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
            selection: None,
            last_csi: None,
            history_prompt_y: None,
            history_clear_timeout: 0,
            history_printed_lines: std::collections::HashSet::new(),
        }

    }

    pub fn has_selection(&self) -> bool {
        self.selection.is_some()
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

    pub fn get_selected_text(&self) -> String {
        let sel = match self.selection {
            Some(s) => s,
            None => return String::new(),
        };

        let (mut start_x, mut start_y) = sel.start;
        let (mut end_x, mut end_y) = sel.end;

        // Normalizing: ensure start is before end
        if start_y > end_y || (start_y == end_y && start_x > end_x) {
            std::mem::swap(&mut start_x, &mut end_x);
            std::mem::swap(&mut start_y, &mut end_y);
        }

        let mut result = String::new();
        for y in start_y..=end_y {
            let line = if y < self.history.len() {
                &self.history[y]
            } else if y < self.history.len() + self.grid.len() {
                &self.grid[y - self.history.len()]
            } else {
                continue;
            };

            let x_start = if y == start_y { start_x } else { 0 };
            let x_end = if y == end_y { end_x } else { line.cells.len().saturating_sub(1) };

            for x in x_start..=x_end {
                if x < line.cells.len() {
                    let ch = line.cells[x].ch;
                    if ch != '\0' {
                        result.push(ch);
                    }
                }
            }
            if y < end_y && !line.is_wrapped {
                result.push('\n');
            }
        }
        result
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
                    // --- Wide-aware Cleanup: 만약 시작점이 와이드 캐릭터의 뒷부분(\0)이면 이전 칸도 지움 ---
                    if self.cursor_x > 0 && line.cells[self.cursor_x].ch == '\0' {
                        line.cells[self.cursor_x - 1] = blank;
                    }
                    for x in self.cursor_x..self.cols { line.cells[x] = blank; }
                    if self.cursor_x == 0 { line.is_wrapped = false; }
                }
                1 => { // To start
                    // --- Wide-aware Cleanup: 만약 끝점이 와이드 캐릭터의 본체이면 다음 칸(\0)도 지움 ---
                    for x in 0..=self.cursor_x { if x < self.cols { line.cells[x] = blank; } }
                    if self.cursor_x + 1 < self.cols && line.cells[self.cursor_x + 1].ch == '\0' {
                        line.cells[self.cursor_x + 1] = blank;
                    }
                }
                2 => { // All
                    for x in 0..self.cols { line.cells[x] = blank; }
                    line.is_wrapped = false;
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

            let mut i = 0;
            let mut chunk_count = 0;
            while i < logical.len() {
                let mut new_line = Line::new(new_cols);
                let mut added = 0;
                while added < new_cols && i < logical.len() {
                    let cell = logical[i];
                    let w = if cell.ch == '\0' { 1 } else { unicode_width::UnicodeWidthChar::width(cell.ch).unwrap_or(1) };
                    
                    if w > 1 && added == new_cols - 1 {
                        // Wide char won't fit at the end of the line
                        break;
                    }
                    
                    new_line.cells[added] = cell;
                    added += w;
                    i += 1;
                }
                new_line.is_wrapped = chunk_count > 0;
                new_all_lines.push(new_line);
                chunk_count += 1;
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
        let w = c.width().unwrap_or(1);

        // 1. 래핑 처리: 현재 X 위치 + 글자 너비가 가로 길이를 초과하면 다음 줄로
        if emu.cursor_x + w > emu.cols {
            emu.cursor_x = 0;
            let (top, bottom) = emu.scrolling_region;
            if emu.cursor_y < bottom {
                emu.cursor_y += 1;
                emu.clear_line(emu.cursor_y, 0); // 래핑 시 잔상 제거
                emu.grid[emu.cursor_y].is_wrapped = true;
            } else if emu.cursor_y == bottom {
                emu.scroll_up_in_region(top, bottom);
                emu.clear_line(bottom, 0); // 스크롤 시 하단 행 초기화
                emu.grid[bottom].is_wrapped = true;
            } else {
                if emu.cursor_y < emu.rows - 1 {
                    emu.cursor_y += 1;
                }
            }
        }

        if emu.cursor_y < emu.rows && emu.cursor_x < emu.cols {
            if emu.history_clear_timeout > 0 {
                emu.history_printed_lines.insert(emu.cursor_y);
            }
            
            let line = &mut emu.grid[emu.cursor_y];

            // --- Wide-aware Cleanup: Overwriting part of a wide character ---
            // 1. 만약 현재 위치에 와이드 캐릭터의 continuation(\0)이 있다면, 이전 칸(본래 글자)을 공백으로
            if line.cells[emu.cursor_x].ch == '\0' && emu.cursor_x > 0 {
                line.cells[emu.cursor_x - 1].ch = ' ';
            }
            // 2. 만약 현재 위치에 이미 와이드 캐릭터가 있고, 출력하려는 글자가 narrow(w=1)하거나 
            //    다른 글자로 대체된다면, 다음 칸(\0)을 공백으로
            if emu.cursor_x + 1 < emu.cols && line.cells[emu.cursor_x + 1].ch == '\0' {
                line.cells[emu.cursor_x + 1].ch = ' ';
            }

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
            b'\x08' => { // Backspace (Cursor Move Left)
                if emu.cursor_x > 0 {
                    emu.cursor_x -= 1;
                } else if emu.cursor_y > 0 && emu.grid[emu.cursor_y].is_wrapped {
                    // 이전 줄의 끝으로 래핑 (Reverse Wrap)
                    emu.cursor_y -= 1;
                    emu.cursor_x = emu.cols - 1;
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
            '@' => { // ICH (Insert Character)
                let n = std::cmp::max(1, param1);
                let y = emu.cursor_y;
                let x = emu.cursor_x;
                if y < emu.grid.len() {
                    let blank = Cell { ch: ' ', fg: emu.current_fg, bg: emu.current_bg, bold: false, italic: false, underline: false };
                    let line = &mut emu.grid[y];
                    
                    // --- Wide-aware Cleanup before shifting ---
                    if x > 0 && line.cells[x].ch == '\0' {
                        line.cells[x-1].ch = ' ';
                        line.cells[x].ch = ' ';
                    }

                    for i in (x + n..emu.cols).rev() {
                        line.cells[i] = line.cells[i - n];
                    }
                    for i in x..std::cmp::min(emu.cols, x + n) {
                        line.cells[i] = blank;
                    }

                    // --- Wide-aware Cleanup after shifting ---
                    if x + n < emu.cols && line.cells[x + n].ch == '\0' {
                        line.cells[x + n].ch = ' ';
                    }
                }
            }
            'X' => { // ECH (Erase Character)
                let n = std::cmp::max(1, param1);
                let y = emu.cursor_y;
                let x = emu.cursor_x;

                // ConPTY 다중 행 잔상 우회 (2): 쉘이 위로 점프한 뒤(history_prompt_y 설정됨), 
                // 프롬프트 좌표보다 아래 행(`y > history_y`)에서 X 명령을 내리는 것은
                // 100% ConPTY가 다중 행 잔상을 치우려다 오작동(글자수 모자람, 위치 삑사리)하는 것입니다.
                // 이럴 경우 ECH를 "전체 줄 비우기(Clear Line)"로 묻고 더블로 가버립니다.
                // 같은 행(`y == prompt_y`)이거나, 하단 행 중 새 텍스트가 인쇄된 행(`history_printed_lines.contains`)일 때는, 
                // 대체된 짧은 명령(또는 새 멀티행 명령어) 뒤에 남은 기나긴 흔적(오버행)을 지우려다
                // 길이 값(n)을 오산하여 끝쪽 한글 잔상을 남기는 버그이므로, "커서부터 끝까지 완전 소거(EL 0)"로 격상시킵니다!
                // 반면 새 텍스트가 전혀 인쇄되지 않은 하단 행은 순수히 과거 명령어 잔상(`Ghost`)이므로 절대 소거(EL 2)합니다.
                if let Some(prompt_y) = emu.history_prompt_y {
                    if y >= prompt_y {
                        if emu.history_printed_lines.contains(&y) {
                            emu.clear_line(y, 0); // 0: 새 텍스트가 있는 행은 오버행(끝부분)만 완벽히 소거
                        } else {
                            emu.clear_line(y, 2); // 2: 새 텍스트가 없는 빈 깡통 행은 통째로 삭제
                        }
                        emu.last_csi = Some(('X', param1, true));
                        return; // 로직 즉시 종료
                    }
                }

                if y < emu.grid.len() {
                    let blank = Cell { ch: ' ', fg: emu.current_fg, bg: emu.current_bg, bold: false, italic: false, underline: false };
                    let line = &mut emu.grid[y];

                    // --- Wide-aware Cleanup before shifting ---
                    if x > 0 && line.cells[x].ch == '\0' {
                        line.cells[x-1].ch = ' ';
                    }

                    for i in x..emu.cols {
                        if i + n < emu.cols {
                            line.cells[i] = line.cells[i + n];
                        } else {
                            line.cells[i] = blank;
                        }
                    }

                    // --- Wide-aware Cleanup after shifting: 
                    // 만약 현재 위치(x)에 \0가 왔다면, 이전 글자가 사라졌으므로 공백으로 바꿈 ---
                    if x < emu.cols && line.cells[x].ch == '\0' {
                        line.cells[x].ch = ' ';
                    }
                }
            }
            'A' => emu.cursor_y = emu.cursor_y.saturating_sub(std::cmp::max(1, param1)),
            'B' => emu.cursor_y = std::cmp::min(emu.rows - 1, emu.cursor_y + std::cmp::max(1, param1)),
            'C' => { // CUF (Cursor Forward)
                let n = std::cmp::max(1, param1);
                
                // ConPTY 다중 행 반쪽 지우기 에러 보정 로직 (스마트 휴리스틱)
                // 만약 직전에 빈 공간만을 지우려는 무의미한 명령(X n)이 들어온 직후, 
                // 정확히 똑같은 n 값으로 커서를 전진시킨다면 커서 동기화가 풀린 상태에서 뒷공간을 지운 치명적 오류입니다.
                if let Some(('X', prev_n, target_is_empty)) = emu.last_csi {
                    if prev_n == n && target_is_empty {
                        // 명백한 ConPTY 삭제 델타 버그임이 확인되었으므로, 터미널이 능동적으로 해당 행 전체를 지워 잔상을 제거합니다.
                        emu.clear_line(emu.cursor_y, 2);
                    }
                }
                
                emu.cursor_x = std::cmp::min(emu.cols - 1, emu.cursor_x + n);
            }
            'D' => { // CUB (Cursor Backward)
                let n = std::cmp::max(1, param1);
                emu.cursor_x = emu.cursor_x.saturating_sub(n);
            }
            'H' | 'f' => { // CUP, HVP
                let y = param1;
                let x = it.next().and_then(|p| p.first()).copied().unwrap_or(1) as usize;
                
                let old_y = emu.cursor_y;
                let new_y = std::cmp::min(emu.rows - 1, y.saturating_sub(1));
                
                // ConPTY 다중 행 잔상 우회 (1): 위로 점프할 경우, 
                // 프롬프트 좌표(new_y)를 저장하여 이후 하단에 떨어지는 기형적 삭제 명령을 전면 소거로 승격시킬 준비를 합니다.
                if new_y < old_y {
                    emu.history_prompt_y = Some(new_y);
                    emu.history_clear_timeout = 30; // 넉넉히 30번의 CSI 명령 동안만 유효
                    emu.history_printed_lines.clear(); // 이전 출력 기록 초기화
                    
                    // 래핑된 연결 행이 있다면 1차로 모두 소거 (이전 로직 유지)
                    if emu.grid[new_y].is_wrapped {
                        let mut clear_y = new_y + 1;
                        let mut prev_was_wrapped = true;
                        
                        while clear_y < emu.rows && prev_was_wrapped {
                            prev_was_wrapped = emu.grid[clear_y].is_wrapped;
                            emu.clear_line(clear_y, 2);
                            clear_y += 1;
                        }
                    }
                }
                
                emu.cursor_y = new_y;
                emu.cursor_x = std::cmp::min(emu.cols - 1, x.saturating_sub(1));
                emu.grid[emu.cursor_y].is_wrapped = false;
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
                    let blank = Cell { ch: ' ', fg: emu.current_fg, bg: emu.current_bg, bold: false, italic: false, underline: false };
                    let line = &mut emu.grid[y];
                    
                    // --- Wide-aware Cleanup before shifting ---
                    if x > 0 && line.cells[x].ch == '\0' {
                        line.cells[x-1].ch = ' ';
                    }

                    for i in x..emu.cols {
                        if i + n < emu.cols {
                            line.cells[i] = line.cells[i + n];
                        } else {
                            line.cells[i] = blank;
                        }
                    }
                    
                    // --- Wide-aware Cleanup after shifting: 
                    // 만약 현재 위치(x)에 \0가 왔다면, 이전 글자가 사라졌으므로 공백으로 바꿈 ---
                    if x < emu.cols && line.cells[x].ch == '\0' {
                        line.cells[x].ch = ' ';
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
        
        if emu.history_clear_timeout > 0 {
            emu.history_clear_timeout -= 1;
            if emu.history_clear_timeout == 0 {
                emu.history_prompt_y = None;
            }
        }
        
        // Debug logging to a temp file for tracking exactly what the shell sends
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("kterm_debug_csi_v2.log") {
            let mut p_str = String::new();
            for p in params.iter() {
                p_str.push_str(&format!("{:?};", p));
            }
            let _ = writeln!(f, "CSI: action={}, params={} x={} y={} limit_n={}", action, p_str, emu.cursor_x, emu.cursor_y, param1);
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

            let sel = self.selection.map(|s| {
                let (mut start_x, mut start_y) = s.start;
                let (mut end_x, mut end_y) = s.end;
                if start_y > end_y || (start_y == end_y && start_x > end_x) {
                    std::mem::swap(&mut start_x, &mut end_x);
                    std::mem::swap(&mut start_y, &mut end_y);
                }
                ((start_x, start_y), (end_x, end_y))
            });

            for (y, row) in visible_lines.iter().enumerate() {
                let absolute_y = start_idx + y;
                for (x, cell) in row.cells.iter().enumerate() {
                    let pos = Point::new(x as f32 * CHAR_WIDTH, y as f32 * ROW_HEIGHT);
                    
                    // 1. 배경색 렌더링
                    let is_selected = if let Some(((sx, sy), (ex, ey))) = sel {
                        if absolute_y > sy && absolute_y < ey { true }
                        else if absolute_y == sy && absolute_y == ey { x >= sx && x <= ex }
                        else if absolute_y == sy { x >= sx }
                        else if absolute_y == ey { x <= ex }
                        else { false }
                    } else { false };

                    let bg_color = if is_selected {
                        Color::from_rgb(0.3, 0.3, 0.6)
                    } else {
                        cell.bg
                    };

                    if bg_color != Color::BLACK {
                        frame.fill_rectangle(pos, Size::new(CHAR_WIDTH, ROW_HEIGHT), bg_color);
                    }

                    // 2. 텍스트 렌더링
                    if cell.ch != ' ' && cell.ch != '\0' {
                        let mut t = Text::default();
                        t.content = cell.ch.to_string();
                        t.position = pos;
                        t.color = if is_selected { Color::WHITE } else { cell.fg };
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
    on_selection_start: fn(usize, usize) -> Message,
    on_selection_update: fn(usize, usize) -> Message,
    on_right_click: fn() -> Message,
}

impl<'a, Message> TerminalView<'a, Message> {
    pub fn new(
        emulator: &'a TerminalEmulator,
        on_scroll: fn(f32) -> Message,
        on_resize: fn(usize, usize) -> Message,
        on_selection_start: fn(usize, usize) -> Message,
        on_selection_update: fn(usize, usize) -> Message,
        on_right_click: fn() -> Message,
    ) -> Self {
        Self { emulator, on_scroll, on_resize, on_selection_start, on_selection_update, on_right_click }
    }
}

#[derive(Default)]
struct LocalState {
    is_pressed: bool,
}

impl<'a, Message: Clone> Widget<Message, Theme, Renderer> for TerminalView<'a, Message> {
    fn tag(&self) -> tree::Tag { tree::Tag::of::<LocalState>() }
    fn state(&self) -> tree::State { tree::State::new(LocalState::default()) }
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
        tree: &mut Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<LocalState>();
        let bounds = layout.bounds();
        let new_cols = (bounds.width / 10.0) as usize;
        let new_rows = (bounds.height / 20.0) as usize;

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
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                state.is_pressed = true;
                if let Some(cursor_pos) = cursor.position_in(bounds) {
                    let col = (cursor_pos.x / 10.0) as usize;
                    let row = (cursor_pos.y / 20.0) as usize;
                    let start_idx = (self.emulator.history.len() + self.emulator.grid.len()).saturating_sub(self.emulator.rows + self.emulator.display_offset);
                    let absolute_row = start_idx + row;
                    shell.publish((self.on_selection_start)(col, absolute_row));
                }
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.is_pressed = false;
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                shell.publish((self.on_right_click)());
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.is_pressed {
                    if let Some(cursor_pos) = cursor.position_in(bounds) {
                        let col = (cursor_pos.x / 10.0) as usize;
                        let row = (cursor_pos.y / 20.0) as usize;
                        let start_idx = (self.emulator.history.len() + self.emulator.grid.len()).saturating_sub(self.emulator.rows + self.emulator.display_offset);
                        let absolute_row = start_idx + row;
                        shell.publish((self.on_selection_update)(col, absolute_row));
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
