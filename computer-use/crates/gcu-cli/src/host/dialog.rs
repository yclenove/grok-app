//! External approval dialogs. Arguments are argv or child env, never a shell.
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Plain-text prompt. The first row is the default and must be the deny/dismiss choice.
pub struct DialogCopy {
    /// Window title.
    pub title: String,
    /// Body shown to the user.
    pub text: String,
    /// Choice id, then the label. Ids are `turn`, `session`, `always`, `deny`, `allow`, or `dismiss`.
    pub rows: Vec<(String, String)>,
    /// Tool timeout in whole seconds. The Host also enforces its own wait.
    pub timeout_secs: u64,
}

/// One dialog invocation. `program` is never a shell.
pub struct DialogPlan {
    /// Executable name.
    pub program: &'static str,
    /// Argv after the program. User text is an argument, not interpolated into a script.
    pub args: Vec<String>,
    /// Child environment. PowerShell reads labels from here.
    pub env: Vec<(String, String)>,
    /// Ids and labels used to interpret stdout.
    pub rows: Vec<(String, String)>,
}

/// What the dialog tool reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    /// This turn only.
    Turn,
    /// Until the session ends.
    Session,
    /// Persist a grant after this returns.
    Always,
    /// Refuse.
    Deny,
    /// Confirm the sensitive action.
    Allow,
    /// Handoff window closed.
    Dismiss,
    /// The tool's own timeout.
    Timeout,
}

/// Pick zenity, then kdialog, then osascript, then PowerShell. `tools` is a name list, not a live search.
pub fn dialog_plan(tools: &[&str], copy: &DialogCopy) -> Option<DialogPlan> {
    if tools.contains(&"zenity") {
        return Some(zenity(copy));
    }
    if tools.contains(&"kdialog") {
        return Some(kdialog(copy));
    }
    if tools.contains(&"osascript") {
        return Some(osascript(copy));
    }
    if tools.contains(&"powershell") {
        return Some(powershell(copy));
    }
    None
}

pub(crate) async fn ask(copy: &DialogCopy, cancel: &gcu_host_api::Cancel) -> Choice {
    if cancel.is_cancelled() {
        return Choice::Deny;
    }
    let Some(plan) = dialog_plan(&available_tools(), copy) else {
        return Choice::Deny;
    };
    let mut command = Command::new(plan.program);
    command
        .args(&plan.args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    for key in [
        "GCU_DIALOG_TITLE",
        "GCU_DIALOG_BODY",
        "GCU_DIALOG_DENY",
        "GCU_DIALOG_B",
        "GCU_DIALOG_C",
        "GCU_DIALOG_D",
        "GCU_DIALOG_COUNT",
    ] {
        command.env_remove(key);
    }
    for (key, value) in &plan.env {
        command.env(key, value);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return Choice::Deny,
    };
    let mut stdout = child.stdout.take();
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            let _ = child.start_kill();
            Choice::Deny
        }
        status = child.wait() => interpret_status(status, &mut stdout, &plan).await,
    }
}

async fn interpret_status(
    status: std::io::Result<std::process::ExitStatus>,
    stdout: &mut Option<tokio::process::ChildStdout>,
    plan: &DialogPlan,
) -> Choice {
    let code = status.ok().and_then(|status| status.code());
    if code == Some(5) {
        return Choice::Timeout;
    }
    if code != Some(0) {
        return Choice::Deny;
    }
    let text = read_short(stdout).await;
    let trimmed = text.trim();
    if plan.program == "powershell" {
        return interpret_index(trimmed, &plan.rows);
    }
    interpret_label(trimmed, &plan.rows)
}

async fn read_short(stdout: &mut Option<tokio::process::ChildStdout>) -> String {
    let Some(pipe) = stdout.as_mut() else {
        return String::new();
    };
    let mut buf = [0_u8; 64];
    let mut read = Vec::new();
    loop {
        if read.len() >= 256 {
            break;
        }
        match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(count) => read.extend_from_slice(&buf[..count]),
        }
    }
    String::from_utf8_lossy(&read).into_owned()
}

fn interpret_label(text: &str, rows: &[(String, String)]) -> Choice {
    for (id, label) in rows {
        if text == id || text == label {
            return choice_from(id);
        }
    }
    Choice::Deny
}

fn interpret_index(tag: &str, rows: &[(String, String)]) -> Choice {
    let index = match tag {
        "deny" => 0,
        "b" => 1,
        "c" => 2,
        "d" => 3,
        _ => return Choice::Deny,
    };
    rows.get(index)
        .map(|(id, _)| choice_from(id))
        .unwrap_or(Choice::Deny)
}

fn choice_from(id: &str) -> Choice {
    match id {
        "turn" => Choice::Turn,
        "session" => Choice::Session,
        "always" => Choice::Always,
        "allow" => Choice::Allow,
        "dismiss" => Choice::Dismiss,
        _ => Choice::Deny,
    }
}

fn available_tools() -> Vec<&'static str> {
    let mut tools = Vec::new();
    #[cfg(target_os = "linux")]
    {
        if which("zenity") {
            tools.push("zenity");
        }
        if which("kdialog") {
            tools.push("kdialog");
        }
    }
    #[cfg(target_os = "macos")]
    if which("osascript") {
        tools.push("osascript");
    }
    #[cfg(windows)]
    if which("powershell") {
        tools.push("powershell");
    }
    tools
}

fn which(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        if dir.join(name).is_file() {
            return true;
        }
        #[cfg(windows)]
        if dir.join(format!("{name}.exe")).is_file() {
            return true;
        }
    }
    false
}

fn zenity(copy: &DialogCopy) -> DialogPlan {
    let mut args = vec![
        "--list".into(),
        "--radiolist".into(),
        "--title".into(),
        safe(&copy.title),
        "--text".into(),
        safe(&copy.text),
        "--hide-column=2".into(),
        "--print-column=2".into(),
        "--column".into(),
        "pick".into(),
        "--column".into(),
        "id".into(),
        "--column".into(),
        "label".into(),
    ];
    for (index, (id, label)) in copy.rows.iter().enumerate() {
        args.push(if index == 0 { "TRUE" } else { "FALSE" }.into());
        args.push(id.clone());
        args.push(safe(label));
    }
    if copy.timeout_secs > 0 {
        args.push("--timeout".into());
        args.push(copy.timeout_secs.to_string());
    }
    plan("zenity", args, Vec::new(), copy)
}

fn kdialog(copy: &DialogCopy) -> DialogPlan {
    let default = copy
        .rows
        .first()
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| "deny".into());
    let mut args = vec![
        "--title".into(),
        safe(&copy.title),
        "--default".into(),
        default,
        "--menu".into(),
        safe(&copy.text),
    ];
    for (id, label) in &copy.rows {
        args.push(id.clone());
        args.push(safe(label));
    }
    plan("kdialog", args, Vec::new(), copy)
}

fn osascript(copy: &DialogCopy) -> DialogPlan {
    let mut args = vec![
        "-e".into(),
        OSASCRIPT.into(),
        safe(&copy.title),
        safe(&copy.text),
    ];
    for (_, label) in &copy.rows {
        args.push(safe(label));
    }
    plan("osascript", args, Vec::new(), copy)
}

fn powershell(copy: &DialogCopy) -> DialogPlan {
    let args = vec![
        "-NoProfile".into(),
        "-STA".into(),
        "-Command".into(),
        POWERSHELL.into(),
    ];
    let mut env = vec![
        ("GCU_DIALOG_TITLE".into(), safe(&copy.title)),
        ("GCU_DIALOG_BODY".into(), safe(&copy.text)),
        ("GCU_DIALOG_COUNT".into(), copy.rows.len().to_string()),
        ("GCU_DIALOG_DENY".into(), String::new()),
        ("GCU_DIALOG_B".into(), String::new()),
        ("GCU_DIALOG_C".into(), String::new()),
        ("GCU_DIALOG_D".into(), String::new()),
    ];
    for (index, (_, label)) in copy.rows.iter().enumerate() {
        let key = match index {
            0 => "GCU_DIALOG_DENY",
            1 => "GCU_DIALOG_B",
            2 => "GCU_DIALOG_C",
            _ => "GCU_DIALOG_D",
        };
        if let Some(slot) = env.iter_mut().find(|(name, _)| name == key) {
            slot.1 = safe(label);
        }
    }
    plan("powershell", args, env, copy)
}

fn plan(
    program: &'static str,
    args: Vec<String>,
    env: Vec<(String, String)>,
    copy: &DialogCopy,
) -> DialogPlan {
    DialogPlan {
        program,
        args,
        env,
        rows: copy.rows.clone(),
    }
}

fn safe(text: &str) -> String {
    let mut out: String = text.chars().take(1200).collect();
    if out.starts_with('-') {
        out.insert(0, ' ');
    }
    out
}

const OSASCRIPT: &str = r#"on run argv
  set dialogTitle to item 1 of argv
  set dialogText to item 2 of argv
  set labels to items 3 thru -1 of argv
  try
    set picked to choose from list labels with title dialogTitle with prompt dialogText default items {item 1 of labels}
  on error
    return "deny"
  end try
  if picked is false then
    return "deny"
  end if
  return item 1 of picked
end run"#;

const POWERSHELL: &str = r#"Add-Type -AssemblyName System.Windows.Forms
$form = New-Object System.Windows.Forms.Form
$form.Text = $env:GCU_DIALOG_TITLE
$form.TopMost = $true
$form.StartPosition = 'CenterScreen'
$form.Width = 560
$form.Height = 420
$label = New-Object System.Windows.Forms.Label
$label.Text = $env:GCU_DIALOG_BODY
$label.Left = 16
$label.Top = 16
$label.Width = 510
$label.Height = 280
$form.Controls.Add($label)
$deny = New-Object System.Windows.Forms.Button
$deny.Text = $env:GCU_DIALOG_DENY
$deny.Left = 16
$deny.Top = 310
$deny.Width = 120
$deny.Add_Click({ $form.Tag = 'deny'; $form.Close() })
$form.Controls.Add($deny)
$form.AcceptButton = $deny
$form.CancelButton = $deny
if ([int]$env:GCU_DIALOG_COUNT -ge 2) {
  $second = New-Object System.Windows.Forms.Button
  $second.Text = $env:GCU_DIALOG_B
  $second.Left = 146
  $second.Top = 310
  $second.Width = 120
  $second.Add_Click({ $form.Tag = 'b'; $form.Close() })
  $form.Controls.Add($second)
}
if ([int]$env:GCU_DIALOG_COUNT -ge 3) {
  $third = New-Object System.Windows.Forms.Button
  $third.Text = $env:GCU_DIALOG_C
  $third.Left = 276
  $third.Top = 310
  $third.Width = 120
  $third.Add_Click({ $form.Tag = 'c'; $form.Close() })
  $form.Controls.Add($third)
}
if ([int]$env:GCU_DIALOG_COUNT -ge 4) {
  $fourth = New-Object System.Windows.Forms.Button
  $fourth.Text = $env:GCU_DIALOG_D
  $fourth.Left = 406
  $fourth.Top = 310
  $fourth.Width = 120
  $fourth.Add_Click({ $form.Tag = 'd'; $form.Close() })
  $form.Controls.Add($fourth)
}
[void]$form.ShowDialog()
if ($form.Tag) { Write-Output $form.Tag } else { Write-Output 'deny' }
"#;
