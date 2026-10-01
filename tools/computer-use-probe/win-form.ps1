Add-Type -AssemblyName System.Windows.Forms
$form = New-Object System.Windows.Forms.Form
$title = $env:GROK_CU_FIXTURE_TITLE
if (-not $title) { $title = "GrokCuFixture-probe" }
$form.Text = $title
$form.Width = 360
$form.Height = 200
$btn = New-Object System.Windows.Forms.Button
$btn.Text = "Count"
$btn.Width = 100
$btn.Height = 32
$btn.Left = 20
$btn.Top = 20
$script:n = 0
$out = Join-Path $env:TEMP "grok-cu-fixture-clicks.txt"
Set-Content -Path $out -Value "0" -Encoding ascii
$lbl = New-Object System.Windows.Forms.Label
$lbl.Text = "clicks=0"
$lbl.Left = 20
$lbl.Top = 70
$lbl.Width = 300
$btn.Add_Click({
  $script:n++
  $lbl.Text = "clicks=$script:n"
  Set-Content -Path $out -Value "$script:n" -Encoding ascii
})
$form.Controls.Add($btn)
$form.Controls.Add($lbl)
$form.Show()
[System.Windows.Forms.Application]::Run($form)
