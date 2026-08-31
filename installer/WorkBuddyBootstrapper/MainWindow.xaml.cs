using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Forms;

namespace WorkBuddyBootstrapper;

public partial class MainWindow : Window
{
    private readonly InstallerEngine _engine = new();
    private bool _installing;

    public MainWindow()
    {
        InitializeComponent();
        InstallPathTextBox.Text = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "Programs",
            "小亨 WorkBuddy");
    }

    private void Close_Click(object sender, RoutedEventArgs e) => Close();

    private void ToggleCustomize_Click(object sender, RoutedEventArgs e)
    {
        var expanded = CustomizePanel.Visibility != Visibility.Visible;
        CustomizePanel.Visibility = expanded ? Visibility.Visible : Visibility.Collapsed;
        CustomizeButton.Content = expanded ? "收起自定义选项⌃" : "自定义安装⌄";
    }

    private void ChoosePath_Click(object sender, RoutedEventArgs e)
    {
        using var dialog = new FolderBrowserDialog {
            Description = "选择小亨 WorkBuddy 安装位置",
            UseDescriptionForTitle = true,
            SelectedPath = InstallPathTextBox.Text,
        };
        if (dialog.ShowDialog() == System.Windows.Forms.DialogResult.OK) {
            InstallPathTextBox.Text = dialog.SelectedPath;
        }
    }

    private async void Install_Click(object sender, RoutedEventArgs e)
    {
        if (_installing) return;
        if (TermsCheckBox.IsChecked != true) {
            ShowError("请先阅读并同意用户协议和隐私条款。");
            return;
        }
        if (string.IsNullOrWhiteSpace(InstallPathTextBox.Text)) {
            ShowError("请选择有效的安装位置。");
            return;
        }

        _installing = true;
        InstallButton.IsEnabled = false;
        CustomizeButton.IsEnabled = false;
        ErrorText.Visibility = Visibility.Collapsed;
        StatusPanel.Visibility = Visibility.Visible;
        InstallButton.Content = "正在安装…";

        var progress = new Progress<InstallProgress>(UpdateProgress);
        try {
            var options = new InstallerOptions(
                InstallPathTextBox.Text.Trim(),
                DesktopShortcutCheckBox.IsChecked == true,
                StartMenuShortcutCheckBox.IsChecked == true,
                StartupCheckBox.IsChecked == true);
            var appPath = await _engine.InstallAsync(options, progress, CancellationToken.None);
            StatusTitle.Text = "安装完成";
            StatusDetail.Text = "运行环境已配置完成。首次启动时请完成账号、AI 提供商和企业微信授权配置。";
            ProgressText.Text = "100%";
            InstallProgress.Value = 100;
            InstallButton.Content = "立即启动";
            InstallButton.IsEnabled = true;
            InstallButton.Click -= Install_Click;
            InstallButton.Click += (_, _) => _engine.LaunchApplication(appPath);
        } catch (Exception error) {
            ShowError(error.Message);
            StatusTitle.Text = "安装未完成";
            StatusDetail.Text = "未写入不完整的运行时配置。修复问题后可再次运行安装器。";
            InstallButton.Content = "重试安装";
            InstallButton.IsEnabled = true;
            CustomizeButton.IsEnabled = true;
        } finally {
            _installing = false;
        }
    }

    private void UpdateProgress(InstallProgress progress)
    {
        StatusTitle.Text = progress.Title;
        StatusDetail.Text = progress.Detail;
        InstallProgress.Value = progress.Percent;
        ProgressText.Text = $"{progress.Percent}%";
    }

    private void ShowError(string message)
    {
        ErrorText.Text = message;
        ErrorText.Visibility = Visibility.Visible;
    }
}
