using Microsoft.Win32;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Formats.Tar;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Reflection;
using System.Security.Cryptography;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;

namespace WorkBuddyBootstrapper;

public sealed record InstallerOptions(
    string InstallDirectory,
    bool CreateDesktopShortcut,
    bool CreateStartMenuShortcut,
    bool StartWithWindows);

public sealed record InstallProgress(int Percent, string Title, string Detail);

public sealed class InstallerEngine
{
    private const string ProductName = "Harmony Work";
    private const string ApplicationExecutable = "harmony-work.exe";
    private const string LegacyProductName = "小亨 WorkBuddy";
    private const string LegacyRunValueName = "Harmony WorkBuddy";
    private const string LegacyApplicationExecutable = "xiaoheng-workbuddy.exe";
    private const string InnerInstallerName = "harmony-work-setup.exe";
    private readonly string _runtimeRoot = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "Harmony",
        "WorkBuddy",
        "runtime");

    public static string ResolveDefaultInstallDirectory()
    {
        var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        var programs = Path.Combine(localAppData, "Programs");
        var preferred = Path.Combine(programs, ProductName);
        var existingCandidates = new[] {
            preferred,
            Path.Combine(programs, LegacyProductName),
            Path.Combine(localAppData, "xiaoheng-workbuddy"),
        };

        return existingCandidates.FirstOrDefault(path =>
            File.Exists(Path.Combine(path, ApplicationExecutable)) ||
            File.Exists(Path.Combine(path, LegacyApplicationExecutable))) ?? preferred;
    }

    public async Task<string> InstallAsync(
        InstallerOptions options,
        IProgress<InstallProgress> progress,
        CancellationToken cancellationToken)
    {
        progress.Report(new(1, "正在准备安装", ""));
        var payloadRoot = await ResolvePayloadRootAsync(progress, cancellationToken);
        var manifest = await ReadManifestAsync(payloadRoot, cancellationToken);

        progress.Report(new(18, "正在校验安装包", ""));
        await VerifyPayloadAsync(payloadRoot, manifest, cancellationToken);

        progress.Report(new(28, "正在配置必要组件", ""));
        if (!HasNodeRuntime()) InstallNode(payloadRoot);

        progress.Report(new(40, "正在配置必要组件", ""));
        if (!HasPythonRuntime()) await InstallPythonAsync(payloadRoot, cancellationToken);

        progress.Report(new(52, "正在配置必要组件", ""));
        if (!HasGitRuntime()) await InstallPortableGitAsync(payloadRoot, cancellationToken);

        progress.Report(new(64, "正在配置必要组件", ""));
        if (!HasWeComCliRuntime()) InstallWeComCli(payloadRoot);

        progress.Report(new(78, "正在安装 Harmony Work", ""));
        await InstallApplicationAsync(payloadRoot, options.InstallDirectory, cancellationToken);

        var applicationPath = Path.Combine(options.InstallDirectory, ApplicationExecutable);
        if (!File.Exists(applicationPath)) {
            throw new InvalidOperationException($"主程序安装完成后未找到：{applicationPath}");
        }

        progress.Report(new(92, "正在创建快捷方式", ""));
        ConfigureShortcuts(applicationPath, options);
        WriteRuntimeMarker();

        progress.Report(new(100, "安装完成", ""));
        return applicationPath;
    }

    public Task WarmUpAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        using var stream = Assembly.GetExecutingAssembly()
            .GetManifestResourceStream("WorkBuddyBootstrapper.PayloadArchive")
            ?? throw new FileNotFoundException("安装载荷未嵌入安装程序。");
        return Task.CompletedTask;
    }

    public void LaunchApplication(string path)
    {
        Process.Start(new ProcessStartInfo(path) { UseShellExecute = true });
    }

    private static async Task<string> ResolvePayloadRootAsync(
        IProgress<InstallProgress> progress,
        CancellationToken cancellationToken)
    {
        var root = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "Harmony",
            "WorkBuddy",
            "installer-cache",
            InstallerVersion,
            "payload");
        if (File.Exists(Path.Combine(root, "payload.manifest.json"))) return root;

        var staging = root + ".staging-" + Guid.NewGuid().ToString("N");
        var archivePath = Path.Combine(staging, "payload.zip");
        var extractedPayload = Path.Combine(staging, "payload");
        try {
            Directory.CreateDirectory(staging);
            await using var source = Assembly.GetExecutingAssembly().GetManifestResourceStream("WorkBuddyBootstrapper.PayloadArchive")
                ?? throw new FileNotFoundException("安装载荷未嵌入安装程序。");
            await CopyPayloadArchiveAsync(source, archivePath, progress, cancellationToken);
            await ExtractPayloadArchiveAsync(archivePath, extractedPayload, progress, cancellationToken);
            DeleteDirectory(root);
            Directory.CreateDirectory(Path.GetDirectoryName(root)!);
            Directory.Move(extractedPayload, root);
            return root;
        } finally {
            DeleteDirectory(staging);
        }
    }

    private static async Task CopyPayloadArchiveAsync(
        Stream source,
        string archivePath,
        IProgress<InstallProgress> progress,
        CancellationToken cancellationToken)
    {
        await using var target = File.Create(archivePath);
        var total = source.CanSeek ? source.Length : 0;
        var buffer = new byte[1024 * 1024];
        long copied = 0;
        int read;
        while ((read = await source.ReadAsync(buffer.AsMemory(0, buffer.Length), cancellationToken)) > 0) {
            await target.WriteAsync(buffer.AsMemory(0, read), cancellationToken);
            copied += read;
            var percent = total > 0 ? 2 + (int)(copied * 6 / total) : 5;
            progress.Report(new(percent, "正在准备安装", ""));
        }
    }

    private static async Task ExtractPayloadArchiveAsync(
        string archivePath,
        string destination,
        IProgress<InstallProgress> progress,
        CancellationToken cancellationToken)
    {
        await Task.Run(() => {
            using var archive = ZipFile.OpenRead(archivePath);
            var entries = archive.Entries.Where(entry => !string.IsNullOrEmpty(entry.Name)).ToArray();
            var root = Path.GetFullPath(destination) + Path.DirectorySeparatorChar;
            for (var index = 0; index < entries.Length; index++) {
                cancellationToken.ThrowIfCancellationRequested();
                var entry = entries[index];
                var target = Path.GetFullPath(Path.Combine(destination, entry.FullName));
                if (!target.StartsWith(root, StringComparison.OrdinalIgnoreCase)) {
                    throw new InvalidDataException("安装载荷包含非法路径。");
                }
                Directory.CreateDirectory(Path.GetDirectoryName(target)!);
                entry.ExtractToFile(target, overwrite: true);
                var percent = 8 + (int)((index + 1) * 8 / Math.Max(1, entries.Length));
                progress.Report(new(percent, "正在准备安装", ""));
            }
        }, cancellationToken);
    }

    private static async Task<PayloadManifest> ReadManifestAsync(string payloadRoot, CancellationToken cancellationToken)
    {
        var path = Path.Combine(payloadRoot, "payload.manifest.json");
        await using var stream = File.OpenRead(path);
        return await JsonSerializer.DeserializeAsync<PayloadManifest>(stream,
                new JsonSerializerOptions { PropertyNameCaseInsensitive = true }, cancellationToken)
            ?? throw new InvalidDataException("安装载荷清单无效。");
    }

    private static async Task VerifyPayloadAsync(string payloadRoot, PayloadManifest manifest, CancellationToken cancellationToken)
    {
        if (manifest.Files.Count == 0) {
            throw new InvalidDataException("安装载荷清单为空。");
        }
        foreach (var item in manifest.Files) {
            cancellationToken.ThrowIfCancellationRequested();
            var fullPath = Path.GetFullPath(Path.Combine(payloadRoot, item.Path));
            if (!fullPath.StartsWith(Path.GetFullPath(payloadRoot), StringComparison.OrdinalIgnoreCase) || !File.Exists(fullPath)) {
                throw new FileNotFoundException($"缺少安装载荷：{item.Path}");
            }
            var actual = await HashFileAsync(fullPath, cancellationToken);
            if (!actual.Equals(item.Sha256, StringComparison.OrdinalIgnoreCase)) {
                throw new InvalidDataException($"安装载荷校验失败：{item.Path}");
            }
        }
    }

    private void InstallNode(string payloadRoot)
    {
        var zipPath = PayloadPath(payloadRoot, "runtime", "node-v22.14.0-win-x64.zip");
        var staging = CreateStagingDirectory("node");
        try {
            ZipFile.ExtractToDirectory(zipPath, staging, true);
            var source = Directory.GetDirectories(staging).SingleOrDefault()
                ?? throw new InvalidDataException("Node.js 离线包目录结构无效。");
            ReplaceDirectory(source, Path.Combine(_runtimeRoot, "node"));
        } finally {
            DeleteDirectory(staging);
        }
    }

    private async Task InstallPythonAsync(string payloadRoot, CancellationToken cancellationToken)
    {
        var pythonRoot = Path.Combine(_runtimeRoot, "python");
        var installer = PayloadPath(payloadRoot, "runtime", "python-3.12.8-amd64.exe");
        Directory.CreateDirectory(_runtimeRoot);
        await RunProcessAsync(
            installer,
            $"/quiet InstallAllUsers=0 PrependPath=0 Include_pip=1 Include_test=0 TargetDir=\"{pythonRoot}\"",
            cancellationToken);

        var wheelZip = PayloadPath(payloadRoot, "runtime", "harmony-cut-wheelhouse.zip");
        var requirements = PayloadPath(payloadRoot, "runtime", "harmony-cut-requirements.txt");
        var wheelDirectory = Path.Combine(_runtimeRoot, "python-wheelhouse");
        ReplaceDirectoryFromZip(wheelZip, wheelDirectory);
        await RunProcessAsync(
            Path.Combine(pythonRoot, "python.exe"),
            $"-m pip install --disable-pip-version-check --no-index --find-links \"{wheelDirectory}\" -r \"{requirements}\"",
            cancellationToken);
    }

    private async Task InstallPortableGitAsync(string payloadRoot, CancellationToken cancellationToken)
    {
        var gitRoot = Path.Combine(_runtimeRoot, "git");
        DeleteDirectory(gitRoot);
        Directory.CreateDirectory(gitRoot);
        await RunProcessAsync(
            PayloadPath(payloadRoot, "runtime", "PortableGit-2.47.1-64-bit.7z.exe"),
            $"-o\"{gitRoot}\" -y",
            cancellationToken);
    }

    private void InstallWeComCli(string payloadRoot)
    {
        var nodeModules = Path.Combine(_runtimeRoot, "wecom", "node_modules", "@wecom");
        ExtractNpmPackage(
            PayloadPath(payloadRoot, "runtime", "wecom-cli-0.1.9.tgz"),
            Path.Combine(nodeModules, "cli"));
        ExtractNpmPackage(
            PayloadPath(payloadRoot, "runtime", "wecom-cli-win32-x64-0.1.9.tgz"),
            Path.Combine(nodeModules, "cli-win32-x64"));

        var commandPath = Path.Combine(_runtimeRoot, "wecom", "wecom-cli.cmd");
        Directory.CreateDirectory(Path.GetDirectoryName(commandPath)!);
        File.WriteAllText(commandPath,
            "@echo off\r\n\"%~dp0..\\node\\node.exe\" \"%~dp0node_modules\\@wecom\\cli\\bin\\wecom.js\" %*\r\n");
    }

    private static async Task InstallApplicationAsync(string payloadRoot, string installDirectory, CancellationToken cancellationToken)
    {
        Directory.CreateDirectory(installDirectory);
        var installer = PayloadPath(payloadRoot, "app", InnerInstallerName);
        // NSIS /D 必须是最后一个参数，且值不能加引号（即使路径含空格）
        await RunProcessAsync(installer, $"/S /D={installDirectory}", cancellationToken);
    }

    private static void ConfigureShortcuts(string appPath, InstallerOptions options)
    {
        RemoveLegacyShortcuts();
        if (options.CreateDesktopShortcut) {
            CreateShortcut(Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory),
                $"{ProductName}.lnk"), appPath);
        }
        if (options.CreateStartMenuShortcut) {
            CreateShortcut(Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.Programs),
                ProductName,
                $"{ProductName}.lnk"), appPath);
        }

        using var run = Registry.CurrentUser.OpenSubKey(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Run", writable: true);
        if (options.StartWithWindows) {
            run?.SetValue(ProductName, $"\"{appPath}\"");
        } else {
            run?.DeleteValue(ProductName, throwOnMissingValue: false);
        }
        run?.DeleteValue(LegacyRunValueName, throwOnMissingValue: false);
    }

    private static void RemoveLegacyShortcuts()
    {
        var legacyDesktop = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory),
            $"{LegacyProductName}.lnk");
        if (File.Exists(legacyDesktop)) File.Delete(legacyDesktop);

        var legacyStartMenu = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.Programs),
            LegacyProductName);
        DeleteDirectory(legacyStartMenu);
    }

    private static void CreateShortcut(string shortcutPath, string targetPath)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(shortcutPath)!);
        var shellType = Type.GetTypeFromProgID("WScript.Shell")
            ?? throw new InvalidOperationException("无法创建 Windows 快捷方式。");
        dynamic shell = Activator.CreateInstance(shellType)!;
        dynamic shortcut = shell.CreateShortcut(shortcutPath);
        shortcut.TargetPath = targetPath;
        shortcut.WorkingDirectory = Path.GetDirectoryName(targetPath)!;
        shortcut.IconLocation = targetPath;
        shortcut.Save();
    }

    private void WriteRuntimeMarker()
    {
        Directory.CreateDirectory(_runtimeRoot);
        var marker = new {
            version = InstallerVersion,
            installedAt = DateTimeOffset.UtcNow,
            node = Path.Combine(_runtimeRoot, "node", "node.exe"),
            python = Path.Combine(_runtimeRoot, "python", "python.exe"),
            git = Path.Combine(_runtimeRoot, "git", "bin", "git.exe"),
            wecomCli = Path.Combine(_runtimeRoot, "wecom", "wecom-cli.cmd"),
        };
        File.WriteAllText(
            Path.Combine(_runtimeRoot, "managed-runtime.json"),
            JsonSerializer.Serialize(marker, new JsonSerializerOptions { WriteIndented = true }));
    }

    private bool HasNodeRuntime() => File.Exists(Path.Combine(_runtimeRoot, "node", "node.exe"));

    private bool HasPythonRuntime() =>
        File.Exists(Path.Combine(_runtimeRoot, "python", "python.exe")) &&
        Directory.Exists(Path.Combine(_runtimeRoot, "python-wheelhouse"));

    private bool HasGitRuntime() => File.Exists(Path.Combine(_runtimeRoot, "git", "bin", "git.exe"));

    private bool HasWeComCliRuntime() =>
        File.Exists(Path.Combine(_runtimeRoot, "wecom", "wecom-cli.cmd")) &&
        File.Exists(Path.Combine(_runtimeRoot, "wecom", "node_modules", "@wecom", "cli", "bin", "wecom.js"));

    private static string InstallerVersion =>
        Assembly.GetExecutingAssembly()
            .GetCustomAttribute<AssemblyInformationalVersionAttribute>()?
            .InformationalVersion
            .Split('+')[0]
        ?? "dev";

    private static void ExtractNpmPackage(string packagePath, string destination)
    {
        DeleteDirectory(destination);
        Directory.CreateDirectory(destination);
        using var packageStream = File.OpenRead(packagePath);
        using var gzip = new GZipStream(packageStream, CompressionMode.Decompress);
        using var archive = new TarReader(gzip);
        TarEntry? entry;
        while ((entry = archive.GetNextEntry()) is not null) {
            if (!entry.Name.StartsWith("package/", StringComparison.Ordinal) || entry.EntryType is TarEntryType.Directory) continue;
            var relative = entry.Name["package/".Length..];
            var target = Path.GetFullPath(Path.Combine(destination, relative));
            if (!target.StartsWith(Path.GetFullPath(destination), StringComparison.OrdinalIgnoreCase)) {
                throw new InvalidDataException("WeCom CLI 包包含非法路径。");
            }
            Directory.CreateDirectory(Path.GetDirectoryName(target)!);
            if (entry.DataStream is not null) {
                using var output = File.Create(target);
                entry.DataStream.CopyTo(output);
            }
        }
    }

    private static void ReplaceDirectoryFromZip(string zipPath, string destination)
    {
        var staging = CreateStagingDirectory("wheels");
        try {
            ZipFile.ExtractToDirectory(zipPath, staging, true);
            ReplaceDirectory(staging, destination);
        } catch {
            DeleteDirectory(staging);
            throw;
        }
    }

    private static void ReplaceDirectory(string source, string destination)
    {
        DeleteDirectory(destination);
        Directory.CreateDirectory(Path.GetDirectoryName(destination)!);
        Directory.Move(source, destination);
    }

    private static string CreateStagingDirectory(string name)
    {
        var directory = Path.Combine(Path.GetTempPath(), "Harmony", "WorkBuddySetup", name, Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);
        return directory;
    }

    private static void DeleteDirectory(string path)
    {
        if (Directory.Exists(path)) Directory.Delete(path, recursive: true);
    }

    private static string PayloadPath(string root, params string[] segments)
    {
        var path = segments.Aggregate(root, Path.Combine);
        if (!File.Exists(path)) throw new FileNotFoundException($"缺少安装载荷：{Path.Combine(segments)}", path);
        return path;
    }

    private static async Task<string> HashFileAsync(string path, CancellationToken cancellationToken)
    {
        await using var stream = File.OpenRead(path);
        var hash = await SHA256.HashDataAsync(stream, cancellationToken);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }

    private static async Task RunProcessAsync(string fileName, string arguments, CancellationToken cancellationToken)
    {
        using var process = Process.Start(new ProcessStartInfo(fileName, arguments) {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardError = true,
        }) ?? throw new InvalidOperationException($"无法启动：{fileName}");
        var stderrTask = process.StandardError.ReadToEndAsync(cancellationToken);
        await process.WaitForExitAsync(cancellationToken);
        var stderr = await stderrTask;
        if (process.ExitCode != 0) {
            throw new InvalidOperationException($"安装命令失败（{Path.GetFileName(fileName)}，退出码 {process.ExitCode}）：{stderr.Trim()}");
        }
    }
}

public sealed class PayloadManifest
{
    public string Version { get; set; } = string.Empty;
    public List<PayloadFile> Files { get; set; } = [];
}

public sealed class PayloadFile
{
    public string Path { get; set; } = string.Empty;
    public string Sha256 { get; set; } = string.Empty;
}
