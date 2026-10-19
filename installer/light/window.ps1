function NewBrandedUpdateWindow {
 param([string]$Directory,[object]$Appearance=$null)
$xml=[xml][IO.File]::ReadAllText((Join-Path $Directory 'ProgressWindow.xaml'))
$reader=New-Object Xml.XmlNodeReader $xml
$window=[Windows.Markup.XamlReader]::Load($reader)
$image=New-Object Windows.Media.Imaging.BitmapImage
$image.BeginInit();$image.CacheOption=[Windows.Media.Imaging.BitmapCacheOption]::OnLoad;$image.UriSource=New-Object Uri((Join-Path $Directory 'brand.png'));$image.EndInit();$image.Freeze();$window.FindName('BrandIcon').Source=$image;$window.Icon=$image
if(!$Appearance){try{$Appearance=Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'Harmony\WorkBuddy\updates\appearance.json') -Raw -Encoding UTF8|ConvertFrom-Json}catch{}}
$dark=if($appearance -and $appearance.colorMode -in @('light','dark')){$appearance.colorMode -eq 'dark'}else{(Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize' -ErrorAction SilentlyContinue).AppsUseLightTheme -eq 0}
$gold=$appearance -and $appearance.theme -eq 'gold'
$palette=if($dark){@{Surface='#1C1E2E';Text='#F3F5FF';Muted='#C2C8DC';Line='#35394D';Accent='#8B7CFF';Soft='#2B2D46'}}else{@{Surface='#FFFFFF';Text='#202536';Muted='#667085';Line='#E4E4E8';Accent='#6658F6';Soft='#F3F2FF'}}
if($gold){$palette=if($dark){@{Surface='#111110';Text='#E8E4D8';Muted='#8A8070';Line='#2A2820';Accent='#C9A84C';Soft='#1A1916'}}else{@{Surface='#FFFFFF';Text='#1C1A14';Muted='#6A6050';Line='#DDD6C8';Accent='#C9A84C';Soft='#F0ECE4'}}}
foreach($entry in $palette.GetEnumerator()){$window.Resources[$entry.Key]=[Windows.Media.BrushConverter]::new().ConvertFromString($entry.Value)}

 return $window
}
