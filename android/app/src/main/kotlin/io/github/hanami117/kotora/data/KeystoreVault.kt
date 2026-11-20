package io.github.hanami117.kotora.data

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The tokens at rest: one file per service (`mal.token`, `anilist.token`) in the app's no-backup
 * folder, each AES-256-GCM encrypted with a key that lives in the Android Keystore and never
 * leaves it. A copy of the files (a rooted phone, a stray backup) carries no usable token.
 *
 * A file the key can't open (the key was lost, e.g. after the user removed the screen lock on
 * some phones) reads as signed out: the user signs in again. Never logs a token.
 */
class KeystoreVault(private val dir: File) {
    private fun file(service: String): File {
        require(service.matches(Regex("[a-z]+"))) { "bad service name" }
        return File(dir, "$service.token")
    }

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (ks.getEntry(ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    fun load(service: String): ByteArray? {
        val f = file(service)
        if (!f.exists()) return null
        return try {
            val data = f.readBytes()
            if (data.size <= IV_LEN) return null
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, data, 0, IV_LEN))
            cipher.doFinal(data, IV_LEN, data.size - IV_LEN)
        } catch (e: Exception) {
            Log.w("kotora", "a saved sign-in can't be read (${e.javaClass.simpleName}); sign in again")
            null
        }
    }

    fun save(service: String, bytes: ByteArray): Boolean = try {
        dir.mkdirs()
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val iv = cipher.iv
        check(iv.size == IV_LEN)
        val tmp = File(dir, "${file(service).name}.tmp")
        tmp.writeBytes(iv + cipher.doFinal(bytes))
        tmp.renameTo(file(service))
    } catch (e: Exception) {
        Log.w("kotora", "the sign-in can't be saved (${e.javaClass.simpleName})")
        false
    }

    fun delete(service: String) {
        file(service).delete()
    }

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "kotora-tokens"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_LEN = 12
        const val TAG_BITS = 128
    }
}
