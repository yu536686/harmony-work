using System;
using System.Diagnostics;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Forms;
using System.Windows.Navigation;

namespace WorkBuddyBootstrapper;

public partial class MainWindow : Window
{
    private readonly InstallerEngine _engine = new();
    private bool _installing;
    private bool _loadFailed;
    private bool _closeAfterCancellation;
    private string? _installedApplicationPath;
    private CancellationTokenSource? _installCancellation;

    public MainWindow()
    {
        InitializeComponent();
        InstallPathTextBox.Text = InstallerEngine.ResolveDefaultInstallDirectory();
        Loaded += Window_Loaded;
    }

    private async void Window_Loaded(object sender, RoutedEventArgs e)
    {
        StatusPanel.Visibility = Visibility.Visible;
        StatusTitle.Text = "正在加载安装器";
        ProgressText.Text = "";
        InstallProgress.IsIndeterminate = true;
        try {
            await _engine.WarmUpAsync(CancellationToken.None);
            StatusTitle.Text = "已准备就绪";
            ProgressText.Text = "100%";
            InstallProgress.IsIndeterminate = false;
            InstallProgress.Value = 100;
            InstallButton.Content = "立即安装";
            InstallButton.IsEnabled = true;
        } catch (Exception error) {
            _loadFailed = true;
            InstallProgress.IsIndeterminate = false;
            StatusTitle.Text = "安装器加载失败";
            ShowError(error.Message);
            InstallButton.Content = "关闭";
            InstallButton.IsEnabled = true;
        }
    }

    private void Close_Click(object sender, RoutedEventArgs e)
    {
        if (_installing) {
            _closeAfterCancellation = true;
            _installCancellation?.Cancel();
            StatusTitle.Text = "正在取消安装";
            return;
        }
        System.Windows.Application.Current.Shutdown();
    }

    private void LegalLink_RequestNavigate(object sender, RequestNavigateEventArgs e)
    {
        e.Handled = true;
        try {
            Process.Start(new ProcessStartInfo(e.Uri.AbsoluteUri) { UseShellExecute = true });
        } catch (Exception error) {
            ShowError($"无法打开浏览器：{error.Message}");
        }
    }

    private void ToggleCustomize_Click(object sender, RoutedEventArgs e)
    {
        var expanded = CustomizePanel.Visibility != Visibility.Visible;
        CustomizePanel.Visibility = expanded ? Visibility.Visible : Visibility.Collapsed;
        CustomizeButton.Content = expanded ? "收起自定义选项⌃" : "自定义安装⌄";
    }

    private void ChoosePath_Click(object sender, RoutedEventArgs e)
    {
        using var dialog = new FolderBrowserDialog {
            Description = "选择 Harmony Work 安装位置",
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
        if (_loadFailed) {
            System.Windows.Application.Current.Shutdown();
            return;
        }
        if (!string.IsNullOrWhiteSpace(_installedApplicationPath)) {
            LaunchAndExit(_installedApplicationPath);
            return;
        }
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
        _installCancellation = new CancellationTokenSource();
        try {
            var options = new InstallerOptions(
                InstallPathTextBox.Text.Trim(),
                DesktopShortcutCheckBox.IsChecked == true,
                StartMenuShortcutCheckBox.IsChecked == true,
                StartupCheckBox.IsChecked == true);
            var appPath = await _engine.InstallAsync(options, progress, _installCancellation.Token);
            StatusTitle.Text = "安装完成";
            ProgressText.Text = "100%";
            InstallProgress.Value = 100;
            _installedApplicationPath = appPath;
            InstallButton.Content = "立即启动";
            InstallButton.IsEnabled = true;
        } catch (OperationCanceledException) when (_closeAfterCancellation) {
            StatusTitle.Text = "已取消安装";
        } catch (Exception error) {
            ShowError(error.Message);
            StatusTitle.Text = "安装未完成";
            InstallButton.Content = "重试安装";
            InstallButton.IsEnabled = true;
            CustomizeButton.IsEnabled = true;
        } finally {
            _installCancellation?.Dispose();
            _installCancellation = null;
            _installing = false;
            if (_closeAfterCancellation) {
                System.Windows.Application.Current.Shutdown();
            }
        }
    }

    private void UpdateProgress(InstallProgress progress)
    {
        StatusTitle.Text = progress.Title;
        StatusDetail.Text = progress.Detail;
        InstallProgress.Value = progress.Percent;
        ProgressText.Text = $"{progress.Percent}%";
    }

    private void LaunchAndExit(string appPath)
    {
        try {
            _engine.LaunchApplication(appPath);
            System.Windows.Application.Current.Shutdown();
        } catch (Exception error) {
            ShowError($"无法启动 Harmony Work：{error.Message}");
        }
    }

    private void ShowError(string message)
    {
        ErrorText.Text = message;
        ErrorText.Visibility = Visibility.Visible;
    }
}
