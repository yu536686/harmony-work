using Microsoft.Win32;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;

namespace HarmonyWorkUninstaller;

public sealed record UninstallProgress(int Percent, string Message);
public sealed record UninstallResult(int RemovedPaths, IReadOnlyList<string> RetainedPaths, IReadOnlyList<string> Errors);

public sealed class UninstallEngine
{
    private readonly string _roaming = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
    private readonly string _local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
    private readonly string _home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);

    public IReadOnlyList<string> ExistingDataRoots() => CleanupPolicy.DataRoots(_roaming, _local)
        .Where(Directory.Exists).ToArray();

    public IReadOnlyList<string> ProtectedPaths() => CleanupPolicy.ProtectedPaths(_roaming, _local, _home);

    public bool HasRegisteredUninstaller() => FindRegisteredUninstaller() is not null;

    public async Task<UninstallResult> RunAsync(bool deleteData, IProgress<UninstallProgress> progress,
        CancellationToken cancellationToken)
    {
        // Capture the workspace setting before NSIS potentially removes config.json.
        var protectedPaths = ProtectedPaths();
        var errors = new List<string>();
        var retained = new List<string>();
        var removed = 0;

        progress.Report(new(8, "正在卸载主程序"));
        var uninstaller = FindRegisteredUninstaller();
        if (uninstaller is not null) {
            var start = new ProcessStartInfo(uninstaller) {
                UseShellExecute = true,
            };
            start.ArgumentList.Add("/S");
            using var process = Process.Start(start)
                ?? throw new InvalidOperationException("无法启动主程序卸载程序。");
            await process.WaitForExitAsync(cancellationToken);
            if (process.ExitCode != 0) {
                throw new InvalidOperationException($"主程序卸载失败（代码 {process.ExitCode}），未清理用户数据。");
            }
            await WaitForUninstallAsync(uninstaller, cancellationToken);
        }

        progress.Report(new(55, "正在清理程序入口"));
        RemoveAppShortcuts(errors, ref removed);

        if (deleteData) {
            var runtime = Path.Combine(_local, "Harmony", "WorkBuddy", "runtime");
            Directory.CreateDirectory(runtime);
            File.WriteAllText(Path.Combine(runtime, ".full-uninstall-cleaned"), "1");
            var roots = CleanupPolicy.DataRoots(_roaming, _local);
            for (var index = 0; index < roots.Count; index++) {
                cancellationToken.ThrowIfCancellationRequested();
                var root = roots[index];
                progress.Report(new(60 + (index * 12), "正在清理程序数据"));
                if (Directory.Exists(root)) {
                    removed += CleanupPolicy.CleanOwnedRoot(root, protectedPaths, retained, errors, cancellationToken);
                }
            }
            var legacyMarker = Path.Combine(_roaming, ".harmony-work-legacy-imported");
            if (File.Exists(legacyMarker)) { File.Delete(legacyMarker); removed++; }
        }

        progress.Report(new(100, errors.Count == 0 ? "卸载完成" : "卸载完成，部分残留需手动处理"));
        return new(removed, retained.Distinct(StringComparer.OrdinalIgnoreCase).ToArray(), errors);
    }

    private static string? FindRegisteredUninstaller()
    {
        foreach (var hive in new[] { RegistryHive.CurrentUser, RegistryHive.LocalMachine }) {
            foreach (var view in new[] { RegistryView.Registry64, RegistryView.Registry32 }) {
                using var baseKey = RegistryKey.OpenBaseKey(hive, view);
                using var uninstall = baseKey.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Uninstall");
                if (uninstall is null) continue;
                foreach (var name in uninstall.GetSubKeyNames()) {
                    using var key = uninstall.OpenSubKey(name);
                    if (key is null) continue;
                    if (!string.Equals(key.GetValue("DisplayName") as string, "Harmony Work",
                            StringComparison.OrdinalIgnoreCase)) continue;
                    var command = key.GetValue("UninstallString") as string;
                    var path = ExtractExecutable(command);
                    if (path is not null && Path.GetFileName(path).Equals("uninstall.exe", StringComparison.OrdinalIgnoreCase)
                        && File.Exists(path)) return path;
                }
            }
        }
        return null;
    }

    private static string? ExtractExecutable(string? command)
    {
        if (string.IsNullOrWhiteSpace(command)) return null;
        command = command.Trim();
        if (command.StartsWith('"')) {
            var end = command.IndexOf('"', 1);
            return end > 1 ? command[1..end] : null;
        }
        var exe = command.IndexOf(".exe", StringComparison.OrdinalIgnoreCase);
        return exe < 0 ? null : command[..(exe + 4)];
    }

    private static async Task WaitForUninstallAsync(string path, CancellationToken cancellationToken)
    {
        for (var attempt = 0; attempt < 90; attempt++) {
            if (!File.Exists(path)) return;
            await Task.Delay(500, cancellationToken);
        }
        throw new InvalidOperationException("主程序卸载未完成，已停止残留清理。请关闭 Harmony Work 后重试。");
    }

    private static void RemoveAppShortcuts(List<string> errors, ref int removed)
    {
        var shortcuts = new[] {
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory), "Harmony Work.lnk"),
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs), "Harmony Work", "Harmony Work.lnk"),
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs), "Harmony Work", "卸载 Harmony Work.lnk"),
        };
        foreach (var path in shortcuts) {
            try {
                if (File.Exists(path)) { File.Delete(path); removed++; }
            } catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) {
                errors.Add($"{path}: {ex.Message}");
            }
        }
        var menu = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs), "Harmony Work");
        if (Directory.Exists(menu) && !Directory.EnumerateFileSystemEntries(menu).Any()) Directory.Delete(menu);
        try {
            using var run = Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Run", writable: true);
            if (run?.GetValue("Harmony Work") is not null) { run.DeleteValue("Harmony Work", false); removed++; }
        } catch (Exception ex) when (ex is UnauthorizedAccessException or System.Security.SecurityException) {
            errors.Add($"开机启动项: {ex.Message}");
        }
    }
}
