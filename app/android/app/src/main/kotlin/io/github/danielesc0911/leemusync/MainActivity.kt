package io.github.danielesc0911.leemusync

import android.os.Bundle
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        Bridge.initTls(applicationContext)
        super.onCreate(savedInstanceState)
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        // SP1 probe only (app/tool/sp1_probe.dart): calls UniFFI from Kotlin.
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "leemusync/sp1")
            .setMethodCallHandler { call, result ->
                if (call.method == "enginePing") {
                    result.success(uniffi.leemusync.enginePing())
                } else {
                    result.notImplemented()
                }
            }
    }
}
