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
#[derive(Clone, Copy)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
}

// ─── 터미널 상태 ──────────────────────────────────────────────────────────────
pub struct TerminalEmulator {
    pub cols: usize,
    pub rows: usize,
    pub grid: Vec<Vec<Cell>>,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub current_fg: Color,
    pub current_bg: Color,
    pub cache: Cache,
    pub parser: Parser,
    pub ime_preedit: String,
}

impl Default for TerminalEmulator {
    fn default() -> Self { Self::new(24, 80) }
}

impl TerminalEmulator {
    pub fn new(rows: usize, cols: usize) -> Self {
        let blank = Cell { ch: ' ', fg: Color::WHITE, bg: Color::BLACK };
        Self {
            cols, rows,
            grid: vec![vec![blank; cols]; rows],
            cursor_x: 0, cursor_y: 0,
            current_fg: Color::WHITE,
            current_bg: Color::BLACK,
            cache: Cache::default(),
            parser: Parser::new(),
            ime_preedit: String::new(),
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

    fn scroll_up(&mut self) {
        let blank = Cell { ch: ' ', fg: Color::WHITE, bg: Color::BLACK };
        if !self.grid.is_empty() {
            self.grid.remove(0);
            self.grid.push(vec![blank; self.cols]);
        }
    }

    fn clear_line(&mut self, row: usize, mode: usize) {
        let blank = Cell { ch: ' ', fg: self.current_fg, bg: self.current_bg };
        if row < self.grid.len() {
            match mode {
                0 => { // To end
                    for x in self.cursor_x..self.cols { self.grid[row][x] = blank; }
                }
                1 => { // To start
                    for x in 0..=self.cursor_x { if x < self.cols { self.grid[row][x] = blank; } }
                }
                2 => { // All
                    for x in 0..self.cols { self.grid[row][x] = blank; }
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

// ─── vte::Perform ─────────────────────────────────────────────────────────────
struct TerminalPerformer<'a> { emulator: &'a mut TerminalEmulator }

impl<'a> Perform for TerminalPerformer<'a> {
    fn print(&mut self, c: char) {
        let emu = &mut self.emulator;

        // 1. 래핑 처리: 현재 X 위치가 가득 찼으면 다음 줄로
        if emu.cursor_x >= emu.cols {
            emu.cursor_x = 0;
            if emu.cursor_y < emu.rows - 1 {
                emu.cursor_y += 1;
            } else {
                emu.scroll_up();
            }
        }

        let w = c.width().unwrap_or(1);
        if emu.cursor_y < emu.rows && emu.cursor_x < emu.cols {
            emu.grid[emu.cursor_y][emu.cursor_x] = Cell { ch: c, fg: emu.current_fg, bg: emu.current_bg };
            if w > 1 && emu.cursor_x + 1 < emu.cols {
                emu.grid[emu.cursor_y][emu.cursor_x + 1] = Cell { ch: '\0', fg: emu.current_fg, bg: emu.current_bg };
            }
            emu.cursor_x += w;
        }
    }

    fn execute(&mut self, byte: u8) {
        let emu = &mut self.emulator;
        match byte {
            b'\n' => { 
                emu.cursor_x = 0;
                if emu.cursor_y < emu.rows - 1 { 
                    emu.cursor_y += 1; 
                } else { 
                    emu.scroll_up(); 
                }
            }
            b'\r'   => emu.cursor_x = 0,
            b'\x08' => { // Backspace
                if emu.cursor_x > 0 {
                    emu.cursor_x -= 1;
                    if emu.grid[emu.cursor_y][emu.cursor_x].ch == '\0' && emu.cursor_x > 0 {
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
            'm' => { // SGR - Colors
                for param in params.iter() {
                    for &p in param {
                        match p {
                            0 => { emu.current_fg = Color::WHITE; emu.current_bg = Color::BLACK; }
                            30..=37 => emu.current_fg = ansi_color((p - 30) as u8),
                            39 => emu.current_fg = Color::WHITE,
                            40..=47 => emu.current_bg = ansi_color((p - 40) as u8),
                            49 => emu.current_bg = Color::BLACK,
                            _ => {}
                        }
                    }
                }
            }
            'J' => emu.clear_screen(param1), // ED
            'K' => emu.clear_line(emu.cursor_y, param1), // EL
            _ => {}
        }
    }
}

// ─── Canvas Program (렌더링) ───────────────────────────────────────────────────
impl<Message> Program<Message> for TerminalEmulator {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: Cursor) -> Vec<Geometry> {
        let geo = self.cache.draw(renderer, bounds.size(), |frame| {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::BLACK);
            for (y, row) in self.grid.iter().enumerate() {
                for (x, cell) in row.iter().enumerate() {
                    if cell.ch != ' ' && cell.ch != '\0' {
                        let mut t = Text::default();
                        t.content = cell.ch.to_string();
                        t.position = Point::new(x as f32 * CHAR_WIDTH, y as f32 * ROW_HEIGHT);
                        t.color = cell.fg;
                        t.size = iced::Pixels(ROW_HEIGHT * 0.95);
                        t.font = D2CODING;
                        frame.fill_text(t);
                    }
                }
            }
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
        });
        vec![geo]
    }
}

// ─── 공식 커스텀 Widget ───────────────────────────────────────────────────────
/// Canvas 기반 렌더링 + Widget::update()에서 shell.request_input_method()로
/// OS에 터미널 커서 정확한 픽셀 좌표 전달 → 한글 팝업 위치 정확
pub struct TerminalView<'a> {
    emulator: &'a TerminalEmulator,
}

impl<'a> TerminalView<'a> {
    pub fn new(emulator: &'a TerminalEmulator) -> Self {
        Self { emulator }
    }
}

impl<'a, Message: Clone> Widget<Message, Theme, Renderer> for TerminalView<'a> {
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
        // RedrawRequested 시마다 IME 정보를 프레임워크에 주입
        if matches!(event, iced::Event::Window(iced::window::Event::RedrawRequested(_))) {
            let ime_state = self.emulator.current_input_method(layout.bounds());
            shell.request_input_method(&ime_state);
        }
    }

    fn operate(&mut self, _tree: &mut Tree, _layout: Layout<'_>, _renderer: &Renderer, _op: &mut dyn iced::advanced::widget::Operation<()>) {}

    fn mouse_interaction(&self, _tree: &Tree, _layout: Layout<'_>, _cursor: mouse::Cursor, _viewport: &Rectangle, _renderer: &Renderer) -> mouse::Interaction {
        mouse::Interaction::default()
    }
}

impl<'a, Message: Clone + 'a> From<TerminalView<'a>> for Element<'a, Message> {
    fn from(w: TerminalView<'a>) -> Self { Element::new(w) }
}
