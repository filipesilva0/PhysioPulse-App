//! Native menu bar, following the Apple HIG order:
//! App, File, Edit, View, Go, Window, Help. Every toolbar action also lives here.
//! Custom item ids match the UI's command table in `src/App.tsx`.

use tauri::menu::{AboutMetadata, Menu, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Runtime};

fn item<R: Runtime>(app: &AppHandle<R>, id: &str, text: &str, accel: Option<&str>) -> tauri::Result<tauri::menu::MenuItem<R>> {
    let mut b = MenuItemBuilder::with_id(id, text);
    if let Some(a) = accel {
        b = b.accelerator(a);
    }
    b.build(app)
}

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let app_menu = SubmenuBuilder::new(app, "PhysioPulse")
        .about(Some(AboutMetadata { name: Some("PhysioPulse".into()), ..Default::default() }))
        .item(&item(app, "check_updates", "Check for Updates…", None)?)
        .separator()
        .item(&item(app, "settings", "Settings…", Some("CmdOrCtrl+,"))?)
        .separator()
        .item(&item(app, "lock", "Lock", Some("Ctrl+CmdOrCtrl+Q"))?)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let file = SubmenuBuilder::new(app, "File")
        .item(&item(app, "new_appointment", "New Appointment…", Some("CmdOrCtrl+N"))?)
        .item(&item(app, "new_patient", "New Patient…", Some("CmdOrCtrl+Shift+N"))?)
        .item(&item(app, "record_payment", "Record Payment…", Some("CmdOrCtrl+Shift+P"))?)
        .item(&item(app, "sell_plan", "Sell Package…", None)?)
        .item(&item(app, "new_expense", "New Expense…", None)?)
        .separator()
        .item(&item(app, "export_record", "Export Clinical Record…", None)?)
        .item(&item(app, "reimbursement_pack", "Reimbursement Pack…", None)?)
        .item(&item(app, "discharge_letter", "Discharge Letter…", None)?)
        .item(&item(app, "record_session", "Record Session…", None)?)
        .item(&item(app, "accountant_export", "Accountant Export…", None)?)
        .separator()
        .item(&item(app, "complaints_log", "Complaints Log…", None)?)
        .item(&item(app, "delete_day", "Delete a Day…", None)?)
        .separator()
        .close_window()
        .build()?;

    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        .item(&item(app, "find", "Find…", Some("CmdOrCtrl+F"))?)
        .build()?;

    let view = SubmenuBuilder::new(app, "View")
        .item(&item(app, "toggle_assistant", "Show/Hide Assistant", Some("Alt+CmdOrCtrl+A"))?)
        .separator()
        .fullscreen()
        .build()?;

    let go = SubmenuBuilder::new(app, "Go")
        .item(&item(app, "back", "Back", Some("CmdOrCtrl+["))?)
        .item(&item(app, "forward", "Forward", Some("CmdOrCtrl+]"))?)
        .separator()
        .item(&item(app, "go_today", "Today", Some("CmdOrCtrl+1"))?)
        .item(&item(app, "go_schedule", "Schedule", Some("CmdOrCtrl+2"))?)
        .item(&item(app, "go_patients", "Patients", Some("CmdOrCtrl+3"))?)
        .item(&item(app, "go_finance", "Finance", Some("CmdOrCtrl+4"))?)
        .item(&item(app, "go_team", "Team", Some("CmdOrCtrl+5"))?)
        .build()?;

    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .item(&PredefinedMenuItem::bring_all_to_front(app, None)?)
        .build()?;

    // Only what works: no "PhysioPulse Help" or "Send Feedback" until they exist (audit M12).
    let help = SubmenuBuilder::new(app, "Help")
        .item(&item(app, "shortcuts", "Keyboard Shortcuts", None)?)
        .build()?;

    Menu::with_items(app, &[&app_menu, &file, &edit, &view, &go, &window, &help])
}
