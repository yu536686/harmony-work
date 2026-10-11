using System;
using System.ComponentModel;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;

namespace HarmonyWorkUninstaller;

public partial class MainWindow : Window
{
    private readonly UninstallEngine _engine = new();
    private bool _running;

    public MainWindow()
    {
        InitializeComponent();
        var roots = _engine.ExistingDataRoots();
        LocationsText.Text = roots.Count == 0 ? "未发现现存应用数据。" : string.Join(Environment.NewLine, roots);
        if (!_engine.HasRegisteredUninstaller()) {
            StatusText.Text = "未发现已安装的主程序，仅可清理残留数据";
            UninstallButton.Content = "清理残留数据";
        }
        var workspaces = _engine.ProtectedPaths()
            .Where(path => !path.EndsWith(@"\Harmony\WorkBuddy\runtime", StringComparison.OrdinalIgnoreCase))
            .ToArray();
        ProtectedWorkspaceText.Text = "保留工作区：" + string.Join("；", workspaces);
    }

    private async void Uninstall_Click(object sender, RoutedEventArgs e)
    {
        if (_running) return;
        _running = true;
        UninstallButton.IsEnabled = false;
        DeleteDataCheckBox.IsEnabled = false;
        ErrorText.Text = "";
        var progress = new Progress<UninstallProgress>(value => {
            UninstallProgress.Value = value.Percent;
            StatusText.Text = value.Message;
        });
        try {
            var deleteData = DeleteDataCheckBox.IsChecked == true;
            var result = await Task.Run(() => _engine.RunAsync(deleteData, progress, CancellationToken.None));
            StatusText.Text = result.Errors.Count == 0 ? $"卸载完成，已清理 {result.RemovedPaths} 项" : "卸载完成，部分残留需手动处理";
            ErrorText.Text = result.Errors.Count == 0 ? "" : string.Join(Environment.NewLine, result.Errors.Take(3));
            LocationsText.Text = result.RetainedPaths.Count == 0 ? "程序残留已清理。" :
                "以下受保护位置未清理：" + Environment.NewLine + string.Join(Environment.NewLine, result.RetainedPaths);
            UninstallButton.Content = "关闭";
            UninstallButton.Click -= Uninstall_Click;
            UninstallButton.Click += (_, _) => Close();
        } catch (Exception ex) {
            StatusText.Text = "卸载未完成";
            ErrorText.Text = ex.Message;
        } finally {
            _running = false;
            UninstallButton.IsEnabled = true;
            DeleteDataCheckBox.IsEnabled = true;
        }
    }

    private void Close_Click(object sender, RoutedEventArgs e) => Close();

    protected override void OnClosing(CancelEventArgs e)
    {
        if (_running) e.Cancel = true;
        base.OnClosing(e);
    }
}
