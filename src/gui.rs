use std::sync::Arc;

use eframe::egui;
use egui::Context;

use crate::db::TaskRepo;
use crate::memory::MemoryStore;
use crate::skill::SkillManager;

/// Application state shared between the GUI and the backend.
pub struct AppState {
    pub repo: Arc<TaskRepo>,
    pub memory: MemoryStore,
    pub skills: SkillManager,
}

impl AppState {
    pub fn new(db_path: &str) -> anyhow::Result<Self> {
        let repo = Arc::new(TaskRepo::new(db_path)?);
        let memory = MemoryStore::new(Arc::clone(&repo));
        let skills = SkillManager::new(SkillManager::default_dir());
        Ok(Self {
            repo,
            memory,
            skills,
        })
    }
}

/// Main GUI application window.
pub struct GuiApp {
    state: AppState,
    selected_tab: Tab,
    // Task panel state
    plan_list: Vec<String>,
    selected_plan: Option<String>,
    plan_detail: String,
    // Memory panel state
    memory_query: String,
    memory_results: String,
    // Settings
    settings_db_path: String,
    status_message: String,
}

#[derive(PartialEq)]
enum Tab {
    Tasks,
    Memory,
    Skills,
    Settings,
}

impl GuiApp {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            selected_tab: Tab::Tasks,
            plan_list: Vec::new(),
            selected_plan: None,
            plan_detail: String::new(),
            memory_query: String::new(),
            memory_results: String::new(),
            settings_db_path: "agent.db".to_string(),
            status_message: String::new(),
        }
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // ── Top bar ──
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Plan Executor Agent");
                ui.separator();
                ui.label("v0.1.0");
            });
        });

        // ── Tab bar ──
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.selected_tab, Tab::Tasks, "📋 Tasks");
                ui.selectable_value(&mut self.selected_tab, Tab::Memory, "🧠 Memory");
                ui.selectable_value(&mut self.selected_tab, Tab::Skills, "⚡ Skills");
                ui.selectable_value(&mut self.selected_tab, Tab::Settings, "⚙ Settings");
            });
        });

        // ── Central panel ──
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.selected_tab {
                Tab::Tasks => self.show_tasks_panel(ui),
                Tab::Memory => self.show_memory_panel(ui),
                Tab::Skills => self.show_skills_panel(ui),
                Tab::Settings => self.show_settings_panel(ui),
            }
        });

        // ── Status bar ──
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if !self.status_message.is_empty() {
                    ui.label(&self.status_message);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Plans: {}", self.plan_list.len()));
                });
            });
        });
    }
}

impl GuiApp {
    fn show_tasks_panel(&mut self, ui: &mut egui::Ui) {
        egui::SidePanel::left("task_list").resizable(true).default_width(200.0).show_inside(ui, |ui| {
            ui.heading("Plans");
            ui.separator();
            ui.add_space(4.0);
            if ui.button("🔄 Refresh").clicked() {
                self.refresh_plans();
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for plan_name in &self.plan_list {
                    let selected = self.selected_plan.as_deref() == Some(plan_name);
                    if ui.selectable_label(selected, plan_name).clicked() {
                        self.selected_plan = Some(plan_name.clone());
                    }
                }
            });
        });

        ui.vertical(|ui| {
            ui.heading("Plan Details");
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.plan_detail.is_empty() {
                    ui.label("Select a plan from the left panel.");
                } else {
                    ui.label(&self.plan_detail);
                }
            });
        });
    }

    fn show_memory_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Memory Search");
        ui.horizontal(|ui| {
            ui.label("Query:");
            ui.text_edit_singleline(&mut self.memory_query);
            if ui.button("🔍 Search").clicked() {
                let query = self.memory_query.clone();
                let store = &self.state.memory;
                let rt = tokio::runtime::Runtime::new();
                match rt {
                    Ok(runtime) => {
                        let results = runtime.block_on(async {
                            store.recall(&query, 20).await.unwrap_or_default()
                        });
                        self.memory_results = MemoryStore::format_context(&results);
                        if self.memory_results.is_empty() {
                            self.memory_results = "No memories found.".to_string();
                        }
                    }
                    Err(_) => {
                        self.memory_results = "Failed to create runtime.".to_string();
                    }
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(&self.memory_results);
        });
    }

    fn show_skills_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Skills");
        ui.separator();
        if ui.button("📦 Install Built-in Skills").clicked() {
            match SkillManager::install_builtin_skills() {
                Ok(()) => self.status_message = "Built-in skills installed.".to_string(),
                Err(e) => self.status_message = format!("Error: {e}"),
            }
        }
        ui.add_space(8.0);

        let skills = self.state.skills.list_skills().unwrap_or_default();
        if skills.is_empty() {
            ui.label("No skills installed. Click 'Install Built-in Skills' above.");
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for name in &skills {
                    ui.horizontal(|ui| {
                        ui.label(format!("⚡ {name}"));
                        if ui.button("Show").clicked() {
                            match self.state.skills.load_skill(name) {
                                Ok(skill) => {
                                    let detail = format!(
                                        "Name: {}\nDescription: {}\nSteps: {}",
                                        skill.name,
                                        skill.description,
                                        skill.steps.len()
                                    );
                                    self.plan_detail = detail;
                                }
                                Err(e) => self.status_message = format!("Error: {e}"),
                            }
                        }
                    });
                }
            });
        }
    }

    fn show_settings_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Database Path:");
            ui.text_edit_singleline(&mut self.settings_db_path);
        });
        ui.add_space(8.0);

        if ui.button("Open Skills Directory").clicked() {
            let dir = SkillManager::default_dir();
            if dir.exists() {
                let _ = open::that(&dir);
            } else {
                self.status_message = "Skills directory does not exist yet.".to_string();
            }
        }
    }

    fn refresh_plans(&mut self) {
        // Placeholder — in a full implementation, this would query the TaskRepo
        self.plan_list = vec!["(connect to database to see plans)".to_string()];
    }
}
