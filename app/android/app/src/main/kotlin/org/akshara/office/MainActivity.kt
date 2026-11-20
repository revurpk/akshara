package org.akshara.office

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.speech.RecognitionListener
import android.speech.RecognitionSupport
import android.speech.RecognitionSupportCallback
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    private var speech: SystemSpeech? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        val s = SystemSpeech(this)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "akshara/speech").setMethodCallHandler(s::handle)
        speech = s
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == SystemSpeech.PERMISSION_REQUEST) {
            speech?.permissionResult(grantResults.isNotEmpty() && grantResults[0] == PackageManager.PERMISSION_GRANTED)
        }
    }

    override fun onDestroy() {
        speech?.cancel()
        super.onDestroy()
    }
}

/**
 * The system speech recogniser for voice input (lib/assist/system_speech.dart
 * describes the 'akshara/speech' channel). Only the on-device recogniser of
 * Android 12 and later is used (createOnDeviceSpeechRecognizer), so audio
 * never leaves the phone; older versions report it as unavailable.
 */
class SystemSpeech(private val activity: FlutterActivity) {
    companion object {
        const val PERMISSION_REQUEST = 4711
        private val LOCALES = linkedMapOf("en" to "en-US", "te" to "te-IN", "hi" to "hi-IN", "sa" to "sa-IN")
    }

    private var recognizer: SpeechRecognizer? = null
    private var pendingStart: Pair<String, MethodChannel.Result>? = null
    private var state = "idle"
    private var text = ""
    private var error: String? = null
    private var level = 0.0
    private var started = 0L

    fun handle(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "available" -> available(result)
            "start" -> start(call.argument<String>("language") ?: "auto", result)
            "stop" -> { stop(); result.success(null) }
            "cancel" -> { cancel(); result.success(null) }
            "status" -> result.success(status())
            else -> result.notImplemented()
        }
    }

    private fun onDeviceAvailable(): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && SpeechRecognizer.isOnDeviceRecognitionAvailable(activity)

    private fun available(result: MethodChannel.Result) {
        if (!onDeviceAvailable()) {
            result.success(mapOf("available" to false, "languages" to emptyList<String>()))
            return
        }
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
            // No way to ask which languages are installed: offer them all and
            // let the recogniser report a missing one.
            result.success(mapOf("available" to true, "languages" to LOCALES.keys.toList()))
            return
        }
        val probe = SpeechRecognizer.createOnDeviceSpeechRecognizer(activity)
        var answered = false
        fun answer(value: Map<String, Any>) {
            if (answered) return
            answered = true
            probe.destroy()
            result.success(value)
        }
        probe.checkRecognitionSupport(
            Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH),
            activity.mainExecutor,
            object : RecognitionSupportCallback {
                override fun onSupportResult(support: RecognitionSupport) {
                    val installed = support.installedOnDeviceLanguages
                    val langs = LOCALES.filter { (_, tag) ->
                        installed.any { it.equals(tag, true) || it.substringBefore('-').equals(tag.substringBefore('-'), true) }
                    }.keys.toList()
                    answer(mapOf("available" to true, "languages" to langs))
                }

                override fun onError(error: Int) {
                    answer(mapOf("available" to true, "languages" to LOCALES.keys.toList()))
                }
            },
        )
    }

    private fun start(language: String, result: MethodChannel.Result) {
        cancel()
        if (!onDeviceAvailable()) {
            result.error("unavailable", "This device has no on-device speech recogniser.", null)
            return
        }
        if (activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            pendingStart = language to result
            activity.requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), PERMISSION_REQUEST)
            return
        }
        begin(language, result)
    }

    fun permissionResult(granted: Boolean) {
        val (language, result) = pendingStart ?: return
        pendingStart = null
        if (granted) {
            begin(language, result)
        } else {
            result.error("denied", "Akshara may not use the microphone. Allow it in the system settings.", null)
        }
    }

    private fun begin(language: String, result: MethodChannel.Result) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return
        val r = SpeechRecognizer.createOnDeviceSpeechRecognizer(activity)
        r.setRecognitionListener(listener)
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
            putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)
            LOCALES[language]?.let { putExtra(RecognizerIntent.EXTRA_LANGUAGE, it) }
        }
        recognizer = r
        text = ""
        error = null
        level = 0.0
        started = SystemClock.elapsedRealtime()
        state = "recording"
        r.startListening(intent)
        result.success(null)
    }

    private val listener = object : RecognitionListener {
        override fun onReadyForSpeech(params: Bundle?) {}
        override fun onBeginningOfSpeech() {}
        override fun onBufferReceived(buffer: ByteArray?) {}
        override fun onEvent(eventType: Int, params: Bundle?) {}

        override fun onRmsChanged(rmsdB: Float) {
            // Roughly -2 dB (silence) to 10 dB (loud), mapped to the 0–0.33
            // range of the other engines' RMS level.
            level = ((rmsdB + 2f) / 36f).coerceIn(0f, 0.33f).toDouble()
        }

        override fun onEndOfSpeech() {
            if (state == "recording") state = "transcribing"
        }

        override fun onPartialResults(partialResults: Bundle?) {
            best(partialResults)?.let { text = it }
        }

        override fun onResults(results: Bundle?) {
            best(results)?.let { text = it }
            finish("done")
        }

        override fun onError(code: Int) {
            when (code) {
                SpeechRecognizer.ERROR_NO_MATCH, SpeechRecognizer.ERROR_SPEECH_TIMEOUT -> finish("done")
                SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS -> {
                    error = "Akshara may not use the microphone."
                    finish("error")
                }
                SpeechRecognizer.ERROR_LANGUAGE_NOT_SUPPORTED, SpeechRecognizer.ERROR_LANGUAGE_UNAVAILABLE -> {
                    error = "This language is not installed for offline speech recognition on this device."
                    finish("error")
                }
                else -> {
                    error = "Speech recognition failed (error $code)."
                    finish(if (text.isNotEmpty()) "done" else "error")
                }
            }
        }
    }

    private fun best(bundle: Bundle?): String? =
        bundle?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull()?.takeIf { it.isNotBlank() }

    private fun finish(newState: String) {
        if (state != "recording" && state != "transcribing") return
        state = newState
        recognizer?.destroy()
        recognizer = null
    }

    private fun stop() {
        if (state != "recording") return
        state = "transcribing"
        recognizer?.stopListening()  // the result arrives in onResults
    }

    fun cancel() {
        pendingStart?.second?.success(null)
        pendingStart = null
        recognizer?.cancel()
        recognizer?.destroy()
        recognizer = null
        state = "idle"
    }

    private fun status(): Map<String, Any> = when (state) {
        "recording" -> mapOf("state" to "recording", "level" to level, "seconds" to (SystemClock.elapsedRealtime() - started) / 1000.0)
        "transcribing" -> mapOf("state" to "transcribing")
        "error" -> { state = "idle"; mapOf("state" to "error", "error" to (error ?: "speech recognition failed")) }
        else -> {
            val t = if (state == "done") text else ""
            state = "idle"
            text = ""
            mapOf("state" to "done", "text" to t)
        }
    }
}
