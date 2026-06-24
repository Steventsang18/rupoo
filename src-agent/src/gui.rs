//! GUI module for rupoo — TRAE SOLO design system
//!
//! Design tokens sourced from TRAE SOLO CN.app dark_modern theme:
//!   bg hierarchy: deepest(#181818) → base(#1F1F1F) → elevated(#222222) → hover(#2B2B2B)
//!   accent: #0078D4 (restrained blue, functional only)
//!   text: primary(#CCCCCC) / secondary(#9D9D9D) / disabled(#868686)

#[cfg(feature = "gui")]
pub mod inner {
    use eframe::egui;
    use eframe::egui::Color32;
    use std::sync::{Arc, Mutex};

    use crate::agent::Agent;
    use crate::task::{Plan, PlanStatus};

    // ===================================================================
    // Design tokens — TRAE SOLO dark_modern palette
    // ===================================================================
    mod token {
        use eframe::egui::Color32;

        // Background hierarchy (4-layer depth)
        pub const BG_DEEPEST: Color32   = Color32::from_rgb(24, 24, 24);   // #181818
        pub const BG_BASE: Color32      = Color32::from_rgb(31, 31, 31);   // #1F1F1F
        pub const BG_ELEVATED: Color32  = Color32::from_rgb(34, 34, 34);   // #222222
        pub const BG_HOVER: Color32     = Color32::from_rgb(43, 43, 43);   // #2B2B2B
        pub const BG_INPUT: Color32     = Color32::from_rgb(49, 49, 49);   // #313131

        // Borders
        pub const BORDER_SUBTLE: Color32  = Color32::from_rgb(43, 43, 43);
        pub const BORDER_DEFAULT: Color32 = Color32::from_rgb(60, 60, 60);
        #[allow(dead_code)]
        pub const BORDER_FOCUS: Color32   = Color32::from_rgb(0, 120, 212);

        // Text
        pub const TEXT_PRIMARY: Color32   = Color32::from_rgb(204, 204, 204);
        pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(157, 157, 157);
        pub const TEXT_DISABLED: Color32  = Color32::from_rgb(134, 134, 134);
        #[allow(dead_code)]
        pub const TEXT_PLACEHOLDER: Color32 = Color32::from_rgb(152, 152, 152);

        // Accent (restrained, functional only)
        pub const ACCENT: Color32        = Color32::from_rgb(0, 120, 212);
        #[allow(dead_code)]
        pub const ACCENT_HOVER: Color32  = Color32::from_rgb(2, 110, 193);
        pub const ACCENT_BRIGHT: Color32 = Color32::from_rgb(64, 166, 255);

        // Semantic
        pub const SUCCESS: Color32 = Color32::from_rgb(46, 160, 67);
        pub const WARNING: Color32 = Color32::from_rgb(196, 186, 96);
        pub const ERROR: Color32   = Color32::from_rgb(248, 81, 73);
    }

    // ===================================================================
    // Application state
    // ===================================================================
    pub struct RupooGui {
        #[allow(dead_code)]
        agent: Option<Arc<Mutex<Agent>>>,
        chat_messages: Vec<ChatMessage>,
        input_text: String,
        selected_tab: Tab,
        plans: Vec<Plan>,
        selected_plan_id: Option<String>,
        memory_search_query: String,
        memories: Vec<MemoryItem>,
        skills: Vec<SkillItem>,
        api_key_anthropic: String,
        api_key_openai: String,
        api_key_ollama: String,
        selected_model: String,
        chat_loading: bool,
        config_saved: bool,
        #[allow(dead_code)]
        show_config_toast: bool,
    }

    #[derive(PartialEq, Clone, Copy)]
    enum Tab {
        Chat,
        Plan,
        Memory,
        Skills,
        Config,
    }

    impl Tab {
        fn label(&self) -> &str {
            match self {
                Tab::Chat => "Chat",
                Tab::Plan => "Plans",
                Tab::Memory => "Memory",
                Tab::Skills => "Skills",
                Tab::Config => "Settings",
            }
        }
        fn icon(&self) -> &str {
            match self {
                Tab::Chat => "\u{25E1}",
                Tab::Plan => "\u{2261}",
                Tab::Memory => "\u{25C7}",
                Tab::Skills => "\u{25C9}",
                Tab::Config => "\u{2699}",
            }
        }
        fn all() -> [Tab; 5] {
            [Tab::Chat, Tab::Plan, Tab::Memory, Tab::Skills, Tab::Config]
        }
    }

    struct ChatMessage {
        is_user: bool,
        content: String,
        timestamp: chrono::DateTime<chrono::Local>,
    }

    struct MemoryItem {
        #[allow(dead_code)]
        id: String,
        content: String,
        tags: Vec<String>,
        created_at: chrono::DateTime<chrono::Local>,
    }

    struct SkillItem {
        name: String,
        description: String,
        version: String,
        installed: bool,
    }

    // ===================================================================
    // eframe::App
    // ===================================================================
    impl eframe::App for RupooGui {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.apply_theme(ctx);

            egui::TopBottomPanel::top("titlebar")
                .min_height(36.0)
                .show(ctx, |ui| {
                    self.render_titlebar(ui);
                });

            egui::SidePanel::left("sidebar")
                .min_width(200.0)
                .max_width(240.0)
                .resizable(false)
                .show(ctx, |ui| {
                    self.render_sidebar(ui);
                });

            egui::TopBottomPanel::bottom("statusbar")
                .min_height(22.0)
                .show(ctx, |ui| {
                    self.render_statusbar(ui);
                });

            egui::CentralPanel::default().show(ctx, |ui| {
                self.render_main_content(ui);
            });

            if self.selected_tab == Tab::Chat {
                egui::TopBottomPanel::bottom("chat_input")
                    .min_height(52.0)
                    .show(ctx, |ui| {
                        self.render_chat_input(ui);
                    });
            }
        }
    }

    // ===================================================================
    // Constructor
    // ===================================================================
    impl RupooGui {
        pub fn new(agent: Option<Arc<Mutex<Agent>>>) -> Self {
            Self {
                agent,
                chat_messages: Vec::new(),
                input_text: String::new(),
                selected_tab: Tab::Chat,
                plans: Vec::new(),
                selected_plan_id: None,
                memory_search_query: String::new(),
                memories: Vec::new(),
                skills: Vec::new(),
                api_key_anthropic: String::new(),
                api_key_openai: String::new(),
                api_key_ollama: String::new(),
                selected_model: String::from("claude-3-sonnet"),
                chat_loading: false,
                config_saved: false,
                show_config_toast: false,
            }
        }

        // ===========================================================
        // Theme
        // ===========================================================
        fn apply_theme(&self, ctx: &egui::Context) {
            let mut visuals = egui::Visuals::dark();

            visuals.panel_fill = token::BG_DEEPEST;
            visuals.window_fill = token::BG_DEEPEST;

            visuals.widgets.noninteractive.bg_fill = token::BG_ELEVATED;
            visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, token::TEXT_PRIMARY);
            visuals.widgets.inactive.bg_fill  = token::BG_INPUT;
            visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, token::TEXT_PRIMARY);
            visuals.widgets.hovered.bg_fill   = token::BG_HOVER;
            visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, token::TEXT_PRIMARY);
            visuals.widgets.active.bg_fill    = token::ACCENT;
            visuals.widgets.active.fg_stroke  = egui::Stroke::new(1.0, Color32::WHITE);
            visuals.widgets.open.bg_fill      = token::BG_HOVER;
            visuals.widgets.open.fg_stroke    = egui::Stroke::new(1.0, token::TEXT_PRIMARY);

            visuals.selection.bg_fill = token::ACCENT;
            visuals.selection.stroke  = egui::Stroke::new(1.0, token::ACCENT);
            visuals.hyperlink_color = token::ACCENT_BRIGHT;

            visuals.window_rounding = egui::Rounding::same(6.0);
            visuals.menu_rounding = egui::Rounding::same(4.0);

            ctx.set_visuals(visuals);

            let mut style = (*ctx.style()).clone();
            style.spacing.item_spacing = egui::vec2(8.0, 4.0);
            style.spacing.button_padding = egui::vec2(12.0, 6.0);
            ctx.set_style(style);
        }

        // ===========================================================
        // Title bar
        // ===========================================================
        fn render_titlebar(&mut self, ui: &mut egui::Ui) {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("rupoo").size(13.0).color(token::TEXT_PRIMARY),
                );
                ui.label(
                    egui::RichText::new("v0.4.1").size(10.0).color(token::TEXT_DISABLED),
                );
            });
        }

        // ===========================================================
        // Sidebar
        // ===========================================================
        fn render_sidebar(&mut self, ui: &mut egui::Ui) {
            ui.add_space(8.0);

            ui.label(
                egui::RichText::new("NAVIGATION")
                    .size(10.0)
                    .color(token::TEXT_DISABLED),
            );
            ui.add_space(4.0);

            let tabs = Tab::all();
            for tab in tabs {
                self.sidebar_nav_item(ui, tab);
            }

            ui.add_space(ui.available_height() - 60.0);

            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let (dot_color, label) = if self.chat_loading {
                    (token::ACCENT_BRIGHT, "Connected")
                } else {
                    (token::SUCCESS, "Idle")
                };
                ui.label(egui::RichText::new("\u{25CF}").size(8.0).color(dot_color));
                ui.label(
                    egui::RichText::new(label).size(11.0).color(token::TEXT_SECONDARY),
                );
            });
        }

        fn sidebar_nav_item(&mut self, ui: &mut egui::Ui, tab: Tab) {
            let is_selected = self.selected_tab == tab;
            let bg = if is_selected { token::BG_HOVER } else { Color32::TRANSPARENT };

            let response = ui.add(
                egui::Button::new(
                    egui::RichText::new(format!("  {}  {}", tab.icon(), tab.label())).size(12.0),
                )
                .fill(bg)
                .stroke(egui::Stroke::new(0.0, Color32::TRANSPARENT))
                .min_size(egui::vec2(ui.available_width() - 4.0, 32.0))
                .rounding(4.0),
            );

            if is_selected {
                let rect = response.rect;
                let bar = egui::Rect::from_min_size(rect.left_top(), egui::vec2(2.0, rect.height()));
                ui.painter().rect_filled(bar, 0.0, token::ACCENT);
            }

            if response.clicked() {
                self.selected_tab = tab;
            }
        }

        // ===========================================================
        // Status bar
        // ===========================================================
        fn render_statusbar(&mut self, ui: &mut egui::Ui) {
            // Top border
            let rect = ui.max_rect();
            ui.painter().hline(
                rect.x_range(),
                rect.top(),
                egui::Stroke::new(1.0, token::BORDER_SUBTLE),
            );

            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(self.selected_tab.label())
                        .size(11.0)
                        .color(token::TEXT_DISABLED),
                );

                ui.add_space(ui.available_width() - 160.0);

                ui.label(
                    egui::RichText::new("claude-3-sonnet")
                        .size(11.0)
                        .color(token::TEXT_DISABLED),
                );

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("\u{2302}")
                        .size(10.0)
                        .color(token::TEXT_DISABLED),
                );
                ui.label(
                    egui::RichText::new("rupoo")
                        .size(11.0)
                        .color(token::TEXT_DISABLED),
                );
            });
        }

        // ===========================================================
        // Main content router
        // ===========================================================
        fn render_main_content(&mut self, ui: &mut egui::Ui) {
            ui.painter().rect_filled(ui.max_rect(), 0.0, token::BG_BASE);

            egui::ScrollArea::vertical()
                .id_salt("main_scroll")
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    match self.selected_tab {
                        Tab::Chat => self.render_chat(ui),
                        Tab::Plan => self.render_plan(ui),
                        Tab::Memory => self.render_memory(ui),
                        Tab::Skills => self.render_skills(ui),
                        Tab::Config => self.render_config(ui),
                    }
                    ui.add_space(8.0);
                });
        }

        // ===========================================================
        // Chat
        // ===========================================================
        fn render_chat(&mut self, ui: &mut egui::Ui) {
            if self.chat_messages.is_empty() {
                self.render_chat_empty(ui);
                return;
            }

            ui.add_space(16.0);
            for msg in &self.chat_messages {
                let (is_user, content, ts) = (msg.is_user, msg.content.clone(), msg.timestamp);
                self.render_chat_bubble(ui, is_user, &content, ts);
            }

            if self.chat_loading {
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.spinner();
                    ui.label(
                        egui::RichText::new("Thinking...")
                            .size(12.0)
                            .color(token::TEXT_SECONDARY),
                    );
                });
            }
        }

        fn render_chat_empty(&self, ui: &mut egui::Ui) {
            ui.add_space(ui.available_height() * 0.3);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("rupoo").size(24.0).color(token::TEXT_PRIMARY),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Ask anything — I'll help you build.")
                        .size(13.0)
                        .color(token::TEXT_SECONDARY),
                );
                ui.add_space(24.0);

                let suggestions = [
                    "Analyze this project structure",
                    "Create a new plan for feature X",
                    "Explain the code in src/main.rs",
                    "Search my memories for past tasks",
                ];
                for text in suggestions {
                    ui.add(
                        egui::Button::new(egui::RichText::new(text).size(12.0))
                            .fill(token::BG_ELEVATED)
                            .stroke(egui::Stroke::new(1.0, token::BORDER_DEFAULT))
                            .min_size(egui::vec2(320.0, 32.0))
                            .rounding(4.0),
                    );
                    ui.add_space(6.0);
                }
            });
        }

        fn render_chat_bubble(
            &self,
            ui: &mut egui::Ui,
            is_user: bool,
            content: &str,
            timestamp: chrono::DateTime<chrono::Local>,
        ) {
            let max_width = ui.available_width() * 0.72;
            let align = if is_user {
                egui::Align::RIGHT
            } else {
                egui::Align::LEFT
            };

            ui.with_layout(egui::Layout::top_down(align), |ui| {
                let bg = if is_user {
                    token::BG_ELEVATED
                } else {
                    Color32::TRANSPARENT
                };

                egui::Frame::none()
                    .fill(bg)
                    .rounding(6.0)
                    .show(ui, |ui| {
                        ui.set_max_width(max_width);
                        ui.add_space(8.0);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(content).size(13.0).color(token::TEXT_PRIMARY),
                            )
                            .wrap(),
                        );
                        ui.add_space(8.0);
                    });

                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("{}", timestamp.format("%H:%M")))
                        .size(10.0)
                        .color(token::TEXT_DISABLED),
                );
            });

            ui.add_space(12.0);
        }

        fn render_chat_input(&mut self, ui: &mut egui::Ui) {
            let rect = ui.max_rect();
            ui.painter().hline(
                rect.x_range(),
                rect.top(),
                egui::Stroke::new(1.0, token::BORDER_SUBTLE),
            );

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);

                let input_response = ui.add(
                    egui::TextEdit::singleline(&mut self.input_text)
                        .hint_text("Message rupoo...")
                        .desired_width(ui.available_width() - 108.0)
                        .font(egui::TextStyle::Body),
                );

                let send_enabled = !self.input_text.trim().is_empty();

                let send_btn = egui::Button::new(egui::RichText::new("Send").size(12.0))
                    .fill(if send_enabled {
                        token::ACCENT
                    } else {
                        token::BG_INPUT
                    })
                    .stroke(egui::Stroke::new(0.0, Color32::TRANSPARENT))
                    .min_size(egui::vec2(56.0, 30.0))
                    .rounding(4.0);

                let send_response = if send_enabled {
                    ui.add(send_btn)
                } else {
                    ui.add_enabled(false, send_btn)
                };

                let enter_pressed =
                    ui.input(|i| i.key_pressed(egui::Key::Enter)) && input_response.has_focus();

                if (send_response.clicked() || enter_pressed) && send_enabled {
                    self.chat_messages.push(ChatMessage {
                        is_user: true,
                        content: std::mem::take(&mut self.input_text),
                        timestamp: chrono::Local::now(),
                    });
                    self.chat_loading = true;
                }

                ui.add_space(8.0);
            });
        }

        // ===========================================================
        // Plan
        // ===========================================================
        fn render_plan(&mut self, ui: &mut egui::Ui) {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Plans").size(16.0).color(token::TEXT_PRIMARY),
                );
                ui.add_space(ui.available_width() - 80.0);
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("+ New Plan").size(12.0))
                            .fill(token::ACCENT)
                            .stroke(egui::Stroke::new(0.0, Color32::TRANSPARENT))
                            .min_size(egui::vec2(80.0, 26.0))
                            .rounding(4.0),
                    )
                    .clicked()
                {
                    // TODO: create new plan
                }
            });

            ui.add_space(12.0);

            if self.plans.is_empty() {
                ui.add_space(ui.available_height() * 0.25);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No plans yet")
                            .size(14.0)
                            .color(token::TEXT_SECONDARY),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("Create a plan to automate your tasks.")
                            .size(12.0)
                            .color(token::TEXT_DISABLED),
                    );
                });
                return;
            }

            // Clone data needed to avoid borrow conflicts
            let plan_snapshots: Vec<PlanSnapshot> = self
                .plans
                .iter()
                .map(|p| PlanSnapshot {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    status: p.status.clone(),
                    steps_len: p.steps.len(),
                    created_at: p.created_at,
                })
                .collect();
            let selected_id = self.selected_plan_id.clone();

            ui.columns(2, |cols| {
                egui::ScrollArea::vertical()
                    .id_salt("plan_list")
                    .show(&mut cols[0], |ui| {
                        for snap in &plan_snapshots {
                            let is_sel = selected_id.as_deref() == Some(&snap.id);
                            let bg = if is_sel {
                                token::BG_HOVER
                            } else {
                                Color32::TRANSPARENT
                            };

                            let response = ui.add(
                                egui::Button::new(
                                    egui::RichText::new(format!("  {}", snap.name)).size(12.0),
                                )
                                .fill(bg)
                                .stroke(egui::Stroke::new(0.0, Color32::TRANSPARENT))
                                .min_size(egui::vec2(ui.available_width() - 4.0, 36.0))
                                .rounding(4.0),
                            );

                            if is_sel {
                                let rect = response.rect;
                                let bar = egui::Rect::from_min_size(
                                    rect.left_top(),
                                    egui::vec2(2.0, rect.height()),
                                );
                                ui.painter().rect_filled(bar, 0.0, token::ACCENT);
                            } else {
                                let dot_center =
                                    response.rect.left_center() + egui::vec2(10.0, 0.0);
                                let status_color = status_color(&snap.status);
                                ui.painter().circle_filled(dot_center, 3.0, status_color);
                            }

                            if response.clicked() {
                                self.selected_plan_id = Some(snap.id.clone());
                            }
                        }
                    });

                if let Some(pid) = &selected_id {
                    if let Some(snap) = plan_snapshots.iter().find(|p| &p.id == pid) {
                        render_plan_detail_card(&mut cols[1], snap);
                    }
                } else {
                    cols[1].vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("Select a plan")
                                .size(13.0)
                                .color(token::TEXT_DISABLED),
                        );
                    });
                }
            });
        }

        // ===========================================================
        // Memory
        // ===========================================================
        fn render_memory(&mut self, ui: &mut egui::Ui) {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Memory").size(16.0).color(token::TEXT_PRIMARY),
                );
                ui.add_space(ui.available_width() - 250.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.memory_search_query)
                        .hint_text("Search memories...")
                        .desired_width(160.0)
                        .font(egui::TextStyle::Body),
                );
            });

            ui.add_space(16.0);

            if self.memories.is_empty() {
                ui.add_space(ui.available_height() * 0.25);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No memories stored")
                            .size(14.0)
                            .color(token::TEXT_SECONDARY),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "Conversation memories will appear here as you chat.",
                        )
                        .size(12.0)
                        .color(token::TEXT_DISABLED),
                    );
                });
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt("memory_scroll")
                .show(ui, |ui| {
                    for memory in &self.memories {
                        let (content, tags, created_at) = (
                            memory.content.clone(),
                            memory.tags.clone(),
                            memory.created_at,
                        );
                        render_memory_card(ui, &content, &tags, created_at);
                    }
                });
        }

        // ===========================================================
        // Skills
        // ===========================================================
        fn render_skills(&mut self, ui: &mut egui::Ui) {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Skills").size(16.0).color(token::TEXT_PRIMARY),
                );
                ui.add_space(ui.available_width() - 170.0);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Install Built-in").size(12.0),
                        )
                        .fill(token::ACCENT)
                        .stroke(egui::Stroke::new(0.0, Color32::TRANSPARENT))
                        .rounding(4.0),
                    )
                    .clicked()
                {
                    // TODO: install built-in skills
                }
            });

            ui.add_space(16.0);

            if self.skills.is_empty() {
                ui.add_space(ui.available_height() * 0.25);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No skills installed")
                            .size(14.0)
                            .color(token::TEXT_SECONDARY),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "Install built-in skills or create custom ones.",
                        )
                        .size(12.0)
                        .color(token::TEXT_DISABLED),
                    );
                });
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt("skills_scroll")
                .show(ui, |ui| {
                    for skill in &self.skills {
                        render_skill_card(
                            ui,
                            &skill.name,
                            &skill.description,
                            &skill.version,
                            skill.installed,
                        );
                    }
                });
        }

        // ===========================================================
        // Config
        // ===========================================================
        fn render_config(&mut self, ui: &mut egui::Ui) {
            ui.label(
                egui::RichText::new("Settings").size(16.0).color(token::TEXT_PRIMARY),
            );
            ui.add_space(20.0);

            // Section header
            ui.label(
                egui::RichText::new("API Keys")
                    .size(11.0)
                    .color(token::TEXT_DISABLED),
            );
            ui.add_space(8.0);

            // Inline all config inputs to avoid borrow conflicts
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Anthropic API Key")
                        .size(12.0)
                        .color(token::TEXT_SECONDARY),
                );
                ui.add_space(ui.available_width() - 340.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.api_key_anthropic)
                        .password(true)
                        .hint_text("sk-ant-...")
                        .desired_width(240.0)
                        .font(egui::TextStyle::Body),
                );
            });
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("OpenAI API Key")
                        .size(12.0)
                        .color(token::TEXT_SECONDARY),
                );
                ui.add_space(ui.available_width() - 340.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.api_key_openai)
                        .password(true)
                        .hint_text("sk-...")
                        .desired_width(240.0)
                        .font(egui::TextStyle::Body),
                );
            });
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Ollama Base URL")
                        .size(12.0)
                        .color(token::TEXT_SECONDARY),
                );
                ui.add_space(ui.available_width() - 340.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.api_key_ollama)
                        .hint_text("http://localhost:11434")
                        .desired_width(240.0)
                        .font(egui::TextStyle::Body),
                );
            });

            ui.add_space(20.0);

            // Model section
            ui.label(
                egui::RichText::new("Model")
                    .size(11.0)
                    .color(token::TEXT_DISABLED),
            );
            ui.add_space(8.0);

            let current_model = self.selected_model.clone();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Default Model")
                        .size(12.0)
                        .color(token::TEXT_SECONDARY),
                );
                ui.add_space(ui.available_width() - 280.0);
                egui::ComboBox::from_id_salt("model_select")
                    .selected_text(
                        egui::RichText::new(&current_model)
                            .size(12.0)
                            .color(token::TEXT_PRIMARY),
                    )
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.selected_model,
                            "claude-3-sonnet".into(),
                            "Claude 3 Sonnet",
                        );
                        ui.selectable_value(
                            &mut self.selected_model,
                            "claude-3-opus".into(),
                            "Claude 3 Opus",
                        );
                        ui.selectable_value(
                            &mut self.selected_model,
                            "gpt-4".into(),
                            "GPT-4",
                        );
                        ui.selectable_value(
                            &mut self.selected_model,
                            "gpt-4-turbo".into(),
                            "GPT-4 Turbo",
                        );
                        ui.selectable_value(
                            &mut self.selected_model,
                            "llama3-70b".into(),
                            "Llama 3 70B",
                        );
                    });
            });

            ui.add_space(24.0);

            if ui
                .add(
                    egui::Button::new(egui::RichText::new("Save Settings").size(12.0))
                        .fill(token::ACCENT)
                        .stroke(egui::Stroke::new(0.0, Color32::TRANSPARENT))
                        .min_size(egui::vec2(120.0, 30.0))
                        .rounding(4.0),
                )
                .clicked()
            {
                self.config_saved = true;
            }

            if self.config_saved {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Settings saved.")
                        .size(12.0)
                        .color(token::SUCCESS),
                );
            }
        }
    }

    // ===================================================================
    // Shared helper types & fns (no &self, avoids borrow issues)
    // ===================================================================

    struct PlanSnapshot {
        id: String,
        name: String,
        status: PlanStatus,
        steps_len: usize,
        created_at: chrono::DateTime<chrono::Utc>,
    }

    fn status_color(status: &PlanStatus) -> Color32 {
        match status {
            PlanStatus::Pending => token::WARNING,
            PlanStatus::Running => token::ACCENT_BRIGHT,
            PlanStatus::WaitingForInput => token::WARNING,
            PlanStatus::Completed => token::SUCCESS,
            PlanStatus::Failed => token::ERROR,
        }
    }

    fn render_plan_detail_card(ui: &mut egui::Ui, snap: &PlanSnapshot) {
        egui::Frame::none()
            .fill(token::BG_ELEVATED)
            .rounding(6.0)
            .stroke(egui::Stroke::new(1.0, token::BORDER_SUBTLE))
            .show(ui, |ui| {
                ui.add_space(16.0);

                ui.label(
                    egui::RichText::new(&snap.name)
                        .size(15.0)
                        .color(token::TEXT_PRIMARY),
                );
                ui.add_space(12.0);

                let (status_text, sc) = match snap.status {
                    PlanStatus::Pending => ("Pending", token::WARNING),
                    PlanStatus::Running => ("Running", token::ACCENT_BRIGHT),
                    PlanStatus::WaitingForInput => ("Waiting", token::WARNING),
                    PlanStatus::Completed => ("Completed", token::SUCCESS),
                    PlanStatus::Failed => ("Failed", token::ERROR),
                };
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Status")
                            .size(11.0)
                            .color(token::TEXT_SECONDARY),
                    );
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(status_text).size(12.0).color(sc));
                });

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!("{} steps", snap.steps_len))
                        .size(12.0)
                        .color(token::TEXT_SECONDARY),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Created {}",
                        snap.created_at.format("%Y-%m-%d %H:%M")
                    ))
                    .size(11.0)
                    .color(token::TEXT_DISABLED),
                );

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Button::new(egui::RichText::new("Execute").size(12.0))
                            .fill(token::ACCENT)
                            .stroke(egui::Stroke::new(0.0, Color32::TRANSPARENT))
                            .rounding(4.0),
                    );
                    ui.add(
                        egui::Button::new(egui::RichText::new("Delete").size(12.0))
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(1.0, token::BORDER_DEFAULT))
                            .rounding(4.0),
                    );
                });

                ui.add_space(16.0);
            });
    }

    fn render_memory_card(
        ui: &mut egui::Ui,
        content: &str,
        tags: &[String],
        created_at: chrono::DateTime<chrono::Local>,
    ) {
        egui::Frame::none()
            .fill(token::BG_ELEVATED)
            .rounding(6.0)
            .stroke(egui::Stroke::new(1.0, token::BORDER_SUBTLE))
            .show(ui, |ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(content)
                        .size(13.0)
                        .color(token::TEXT_PRIMARY),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    for tag in tags {
                        ui.label(
                            egui::RichText::new(format!("#{}", tag))
                                .size(11.0)
                                .color(token::ACCENT_BRIGHT),
                        );
                        ui.add_space(6.0);
                    }
                    ui.add_space(ui.available_width() - 100.0);
                    ui.label(
                        egui::RichText::new(format!("{}", created_at.format("%m-%d %H:%M")))
                            .size(10.0)
                            .color(token::TEXT_DISABLED),
                    );
                });
                ui.add_space(12.0);
            });
        ui.add_space(8.0);
    }

    fn render_skill_card(
        ui: &mut egui::Ui,
        name: &str,
        description: &str,
        version: &str,
        installed: bool,
    ) {
        let border_color = if installed {
            token::BORDER_SUBTLE
        } else {
            token::BORDER_DEFAULT
        };

        egui::Frame::none()
            .fill(token::BG_ELEVATED)
            .rounding(6.0)
            .stroke(egui::Stroke::new(1.0, border_color))
            .show(ui, |ui| {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(name)
                            .size(13.0)
                            .color(token::TEXT_PRIMARY),
                    );
                    let (badge_text, badge_color) = if installed {
                        ("installed", token::SUCCESS)
                    } else {
                        ("available", token::TEXT_DISABLED)
                    };
                    ui.label(
                        egui::RichText::new(badge_text)
                            .size(10.0)
                            .color(badge_color),
                    );
                    ui.add_space(ui.available_width() - 80.0);
                    ui.label(
                        egui::RichText::new(format!("v{}", version))
                            .size(11.0)
                            .color(token::TEXT_DISABLED),
                    );
                });
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(description)
                        .size(12.0)
                        .color(token::TEXT_SECONDARY),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let label = if installed { "Run" } else { "Install" };
                    ui.add(
                        egui::Button::new(egui::RichText::new(label).size(12.0))
                            .fill(token::ACCENT)
                            .stroke(egui::Stroke::new(0.0, Color32::TRANSPARENT))
                            .rounding(4.0),
                    );
                });
                ui.add_space(12.0);
            });
        ui.add_space(8.0);
    }

    // ===================================================================
    // Entry point
    // ===================================================================
    pub fn run_gui(agent: Option<Arc<Mutex<Agent>>>) -> Result<(), eframe::Error> {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size(egui::vec2(1100.0, 720.0))
                .with_min_inner_size(egui::vec2(780.0, 520.0)),
            ..Default::default()
        };

        eframe::run_native(
            "rupoo",
            native_options,
            Box::new(|_cc| Ok(Box::new(RupooGui::new(agent)))),
        )
    }
}

#[cfg(not(feature = "gui"))]
pub mod inner {
    use crate::agent::Agent;
    use std::sync::{Arc, Mutex};

    pub fn run_gui(_agent: Option<Arc<Mutex<Agent>>>) -> Result<(), String> {
        Err("GUI feature is not enabled".to_string())
    }
}
