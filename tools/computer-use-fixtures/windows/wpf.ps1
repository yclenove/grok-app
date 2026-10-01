Add-Type -AssemblyName PresentationFramework
Add-Type -AssemblyName PresentationCore
Add-Type -AssemblyName WindowsBase

$title = $env:GROK_CU_WPF_TITLE
if (-not $title) { $title = "GrokCuWpf" }
$dir = $env:GROK_CU_WPF_ORACLE
if (-not $dir) { $dir = [System.IO.Path]::GetTempPath() }
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$script:oracleDir = $dir

function Write-Oracle([string]$name, [string]$value) {
  [System.IO.File]::WriteAllText((Join-Path $script:oracleDir $name), $value)
}

Write-Oracle "clicks.txt" "0"
Write-Oracle "edit.txt" ""
Write-Oracle "scroll.txt" "0"
Write-Oracle "dialog.txt" ""
Write-Oracle "drag.txt" "0"

$window = New-Object System.Windows.Window
$window.Title = $title
$window.Width = 540
$window.Height = 420
$window.WindowStartupLocation = "Manual"
$window.Left = 90
$window.Top = 90
$window.Topmost = $true
$window.ShowActivated = $true
$window.Add_Loaded({
  $window.Activate() | Out-Null
  $window.Focus() | Out-Null
})

$panel = New-Object System.Windows.Controls.StackPanel
$panel.Margin = New-Object System.Windows.Thickness 16

$btn = New-Object System.Windows.Controls.Button
$btn.Content = "Count"
$btn.Width = 100
$btn.Height = 32
$btn.HorizontalAlignment = "Left"
$script:n = 0

$status = New-Object System.Windows.Controls.Label
$status.Content = "clicks=0"

$btn.Add_Click({
  $script:n++
  Write-Oracle "clicks.txt" "$script:n"
  $status.Content = "clicks=$script:n"
})

$edit = New-Object System.Windows.Controls.TextBox
$edit.Height = 28
$edit.Add_TextChanged({
  Write-Oracle "edit.txt" $edit.Text
})

$scroll = New-Object System.Windows.Controls.ScrollViewer
$scroll.Height = 140
$list = New-Object System.Windows.Controls.StackPanel
for ($i = 0; $i -lt 30; $i++) {
  $item = New-Object System.Windows.Controls.TextBlock
  $item.Text = ("item-{0:D2}" -f $i)
  [void]$list.Children.Add($item)
}
$scroll.Content = $list
$scroll.Add_ScrollChanged({
  Write-Oracle "scroll.txt" ([int]$scroll.VerticalOffset).ToString()
})

$ask = New-Object System.Windows.Controls.Button
$ask.Content = "Ask"
$ask.Width = 100
$ask.Height = 32
$ask.HorizontalAlignment = "Left"
$ask.Add_Click({
  $dlg = New-Object System.Windows.Window
  $dlg.Title = "GrokCuWpfDialog"
  $dlg.Width = 280
  $dlg.Height = 140
  $dlg.WindowStartupLocation = "CenterOwner"
  $ok = New-Object System.Windows.Controls.Button
  $ok.Content = "WpfOK"
  $ok.Name = "WpfOK"
  $ok.Width = 80
  $ok.Height = 28
  $ok.HorizontalAlignment = "Center"
  $ok.VerticalAlignment = "Center"
  $ok.IsDefault = $true
  $ok.Add_Click({
    [System.IO.File]::WriteAllText((Join-Path $env:GROK_CU_WPF_ORACLE "dialog.txt"), "ok")
    $this.Parent.Close()
  })
  $dlg.Content = $ok
  $dlg.Show() | Out-Null
})

$drop = New-Object System.Windows.Controls.Border
$drop.Height = 48
$drop.Background = [System.Windows.Media.Brushes]::LightGray
$drop.AllowDrop = $true
$dropLabel = New-Object System.Windows.Controls.TextBlock
$dropLabel.Text = "Drop"
$dropLabel.HorizontalAlignment = "Center"
$dropLabel.VerticalAlignment = "Center"
$drop.Child = $dropLabel
$drop.Add_Drop({
  Write-Oracle "drag.txt" "1"
})

$drag = New-Object System.Windows.Controls.Button
$drag.Content = "Drag"
$drag.Width = 100
$drag.Height = 32
$drag.HorizontalAlignment = "Left"
$drag.Add_PreviewMouseMove({
  param($sender, $e)
  if ($e.LeftButton -eq [System.Windows.Input.MouseButtonState]::Pressed) {
    [System.Windows.DragDrop]::DoDragDrop($drag, "fixture", [System.Windows.DragDropEffects]::Copy) | Out-Null
  }
})

[void]$panel.Children.Add($btn)
[void]$panel.Children.Add($status)
[void]$panel.Children.Add($edit)
[void]$panel.Children.Add($scroll)
[void]$panel.Children.Add($ask)
[void]$panel.Children.Add($drag)
[void]$panel.Children.Add($drop)
$window.Content = $panel
[void]$window.ShowDialog()
