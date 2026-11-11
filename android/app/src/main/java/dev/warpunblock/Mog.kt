package dev.warpunblock

import androidx.compose.material3.LocalTextStyle
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.TextUnit

/**
 * Ренат мод — пасхалка, как в Windows-версии (windows/source/src/mog.rs): фото на фоне,
 * радуга и «моггерские» надписи. В описаниях релизов о нём не пишем.
 */
object Mog {
    var on by mutableStateOf(false)

    private val phrases = mapOf(
        "Связь" to "Джоулайн",
        "Профили" to "Чадодром",
        "Списки" to "Нормисы",
        "Настройки" to "Бонсмэш",
        "Журнал" to "Кортизол",
        "Обход работает" to "Ты могаешь",
        "Работает" to "Могаешь",
        "Отключено" to "Эльдарнулся",
        "Подключено" to "Могаю",
        "Подключаю…" to "Мьюинг…",
        "Отключаю…" to "Эльдарнуться…",
        "Ошибка" to "Фрауд",
        "Нет связи" to "Фрауд",
        "Нажмите, чтобы подключиться" to "Нажмите, чтобы могать",
        "Нажмите, чтобы отключиться" to "Нажмите, чтобы эльдарнуться",
        "Нажмите, чтобы отменить" to "Тапни, чтобы закопить",
        "Время работы" to "Держу мьюинг",
        "Пинг WARP" to "Кантал тилт",
        "Профиль" to "Фрейм",
        "Подбор профиля" to "Кастинг чадов",
        "Трафик через VPN" to "Трафик мьюинга",
        "Проверить обновления" to "Асцендить",
        "Открыть релизы" to "Чё по лору",
        "Ренат мод" to "РЕНАТ МОД 🗿",
        "Оформление" to "Лукмаксинг",
        "Подключение" to "Мьюинг",
        "Приложения в VPN" to "Кого моггаем",
        "Все приложения" to "Всех",
        "Только выбранные" to "Только чадов",
        "Кроме выбранных" to "Всех, кроме нормисов",
        "Системные настройки VPN" to "Настройки ауры у бати",
        "Поиск приложения" to "Найти сабхьюмана",
    )

    /** Отдельные слова: ловят строки движка, журнала и всплывашек. */
    private val words = listOf(
        "Подключено" to "Могаю",
        "подключено" to "могаю",
        "Подключ" to "Мьюин",
        "подключ" to "мьюин",
        "Ошибка" to "Кортизол",
        "ошибка" to "кортизол",
        "Проверяю" to "Меряю джоулайн:",
        "Проверить" to "Рейтнуть по PSL",
        "проверить" to "рейтнуть по PSL",
        "доступен" to "асцендил",
        "работает" to "могает",
        "Сохран" to "Залут",
        "сохран" to "залут",
        "профиль" to "фрейм",
        "WARP" to "WARP-чад",
        "задержка" to "кантал тилт",
        "Не прошёл" to "Не асцендил",
        "прошёл" to "асцендил",
        " мс" to " мс мьюинга",
    )

    fun text(value: String): String {
        if (!on) return value
        phrases[value]?.let { return it }
        var out = value
        for ((from, to) in words) if (from in out) out = out.replace(from, to)
        return out
    }
}

/**
 * Обёртка над material3 Text: в Ренат моде переводит строку. Объявлена в пакете, поэтому
 * перекрывает `androidx.compose.material3.*` во всех файлах приложения.
 */
@Composable
fun Text(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = Color.Unspecified,
    fontSize: TextUnit = TextUnit.Unspecified,
    fontWeight: FontWeight? = null,
    fontFamily: FontFamily? = null,
    textAlign: TextAlign? = null,
    overflow: TextOverflow = TextOverflow.Clip,
    maxLines: Int = Int.MAX_VALUE,
    minLines: Int = 1,
    style: TextStyle = LocalTextStyle.current,
) = androidx.compose.material3.Text(
    Mog.text(text),
    modifier = modifier,
    color = color,
    fontSize = fontSize,
    fontWeight = fontWeight,
    fontFamily = fontFamily,
    textAlign = textAlign,
    overflow = overflow,
    maxLines = maxLines,
    minLines = minLines,
    style = style,
)
