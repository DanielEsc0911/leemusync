// SP1 probe (spike only, not part of the app): calls the Rust bridge, makes
// one HTTPS request with the platform verifier, and measures frame times
// while an animation runs. Prints one `SP1 {json}` line, then exits.
//
//   flutter run --profile -d linux -t tool/sp1_probe.dart
import 'dart:convert';
import 'dart:io';
import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:lemusync/bridge/api/probe.dart';
import 'package:lemusync/bridge/frb_generated.dart';

const _host = String.fromEnvironment(
  'SP1_HOST',
  defaultValue: 'www.rust-lang.org',
);
const _seconds = 10;

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  runApp(const MaterialApp(home: _Probe()));
}

class _Probe extends StatefulWidget {
  const _Probe();

  @override
  State<_Probe> createState() => _ProbeState();
}

class _ProbeState extends State<_Probe> with SingleTickerProviderStateMixin {
  late final AnimationController _spin = AnimationController(
    vsync: this,
    duration: const Duration(seconds: 1),
  )..repeat();
  final List<FrameTiming> _frames = [];
  final Map<String, Object?> _result = {};

  @override
  void initState() {
    super.initState();
    SchedulerBinding.instance.addTimingsCallback(_frames.addAll);
    _run();
  }

  Future<void> _run() async {
    // Dart, then Kotlin/Swift through UniFFI, then Dart again: one shared
    // counter means one library instance in the process.
    _result['first'] = coreVersion();
    if (Platform.isAndroid || Platform.isIOS) {
      _result['native'] = await const MethodChannel('lemusync/sp1')
          .invokeMethod<String>('enginePing');
    }
    _result['second'] = coreVersion();
    try {
      _result['https'] = await httpsStatus(host: _host);
    } on Object catch (e) {
      _result['https_error'] = e.toString();
    }
    setState(() {});
    await Future<void>.delayed(const Duration(seconds: _seconds));
    _result.addAll(_frameStats());
    _result['refresh_hz'] =
        PlatformDispatcher.instance.views.first.display.refreshRate;
    _result['rss_mib'] = ProcessInfo.currentRss / (1 << 20);
    debugPrint('SP1 ${jsonEncode(_result)}');
    exit(0);
  }

  Map<String, Object?> _frameStats() {
    List<double> ms(Duration Function(FrameTiming) f) =>
        _frames.map((t) => f(t).inMicroseconds / 1000).toList()..sort();
    double p(List<double> xs, double q) =>
        xs.isEmpty ? 0 : xs[((xs.length - 1) * q).round()];
    final build = ms((t) => t.buildDuration);
    final raster = ms((t) => t.rasterDuration);
    return {
      'frames': _frames.length,
      'build_p50_ms': p(build, .5),
      'build_p99_ms': p(build, .99),
      'raster_p50_ms': p(raster, .5),
      'raster_p99_ms': p(raster, .99),
      'over_8ms': _frames
          .where((t) => t.totalSpan > const Duration(microseconds: 8333))
          .length,
    };
  }

  @override
  void dispose() {
    _spin.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            RotationTransition(
              turns: _spin,
              child: const FlutterLogo(size: 96),
            ),
            const SizedBox(height: 24),
            Text(
              '${_result['first']} · ${_result['native']} · ${_result['second']}',
            ),
            Text('${_result['https'] ?? _result['https_error'] ?? '…'}'),
          ],
        ),
      ),
    );
  }
}
