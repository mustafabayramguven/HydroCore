# JNI: the Rust library looks these methods up by name.
-keepclasseswithmembernames class * {
    native <methods>;
}
-keep class com.hydrocore.app.engine.NativeTransport { *; }

# kotlinx.serialization models exchanged with the Rust core.
-keepattributes *Annotation*, InnerClasses
-keep,includedescriptorclasses class com.hydrocore.app.domain.**$$serializer { *; }
-keepclassmembers class com.hydrocore.app.domain.** {
    *** Companion;
}
-keepclasseswithmembers class com.hydrocore.app.domain.** {
    kotlinx.serialization.KSerializer serializer(...);
}
